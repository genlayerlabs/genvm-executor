use std::collections::BTreeMap;

use crate::int_traits::*;
use crate::rt::errors;

#[derive(Clone)]
pub struct Archive {
    pub data: BTreeMap<String, bytes::Bytes>,
    pub total_size: u32,
}

use super::malformed_runner_error as malformed_runner;

fn validate_entry_name(name: &str, is_directory: bool) -> errors::Result<()> {
    if name.is_empty() {
        return Err(malformed_runner("entry name is empty"));
    }
    if name.starts_with('/') {
        return Err(malformed_runner(format!(
            "entry name {name:?} starts with a slash"
        )));
    }
    if name.contains('\\') {
        return Err(malformed_runner(format!(
            "entry name {name:?} contains a backslash"
        )));
    }

    let name_without_directory_slash = if is_directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        if name.ends_with('/') {
            return Err(malformed_runner(format!(
                "file entry name {name:?} has a trailing slash"
            )));
        }
        name
    };
    if name_without_directory_slash
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(malformed_runner(format!(
            "entry name {name:?} contains an invalid path component"
        )));
    }

    Ok(())
}

/// Zip's `Stored` method, as it appears in a local file header.
const ZIP_STORED_METHOD: u16 = 0;
const ZIP_LOCAL_HEADER_LEN: usize = 30;
/// General-purpose flag bit 0: the entry is encrypted.
const ZIP_FLAG_ENCRYPTED: u16 = 1 << 0;
/// General-purpose flag bit 3: sizes and CRC-32 trail the data in a descriptor,
/// and the local header carries zeroes instead.
const ZIP_FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;

/// The zip crate reports what the *central directory* says, and reads the local
/// header only to learn where the data begins. An archive whose two copies
/// disagree therefore means different things to different readers: genvm follows
/// the central directory, a streaming reader follows the local header, and a
/// third tool may reject the file outright. Runner ids are content hashes, so
/// that divergence is consensus-visible -- require the two to agree.
///
/// `crc32` and `size` come from the central directory. They are only compared
/// when the entry has no data descriptor: with flag bit 3 set the local header
/// legitimately carries zeroes and the real values trail the data.
fn check_local_header_agrees(
    bytes: &[u8],
    name: &str,
    header_start: u64,
    crc32: u32,
    size: u64,
) -> errors::Result<()> {
    let header_start: usize = header_start.into_int_comptime();
    let header = header_start
        .checked_add(ZIP_LOCAL_HEADER_LEN)
        .and_then(|end| bytes.get(header_start..end))
        .ok_or_else(|| {
            malformed_runner(format!(
                "file {name} local header at {header_start} is out of bounds"
            ))
        })?;

    if header[0..4] != *b"PK\x03\x04" {
        return Err(malformed_runner(format!(
            "file {name} has no local header at {header_start}"
        )));
    }

    let flags = u16::from_le_bytes([header[6], header[7]]);
    if flags & ZIP_FLAG_ENCRYPTED != 0 {
        return Err(malformed_runner(format!("file {name} is encrypted")));
    }

    let method = u16::from_le_bytes([header[8], header[9]]);
    if method != ZIP_STORED_METHOD {
        return Err(malformed_runner(format!(
            "file {name} local compression method {method} contradicts the central directory"
        )));
    }

    // The name is compared as raw bytes rather than after decoding: the crate
    // decodes lossily, so two different byte strings can both arrive as U+FFFD
    // and compare equal. Requiring the local bytes to be valid UTF-8 *and* equal
    // to the central name rejects both a genuine mismatch and a name no two
    // readers would decode alike.
    let name_len: usize = u16::from_le_bytes([header[26], header[27]]).into();
    let local_name = header_start
        .checked_add(ZIP_LOCAL_HEADER_LEN)
        .and_then(|start| Some(start..start.checked_add(name_len)?))
        .and_then(|range| bytes.get(range))
        .ok_or_else(|| {
            malformed_runner(format!(
                "file {name} local name at {header_start} is out of bounds"
            ))
        })?;
    if std::str::from_utf8(local_name) != Ok(name) {
        return Err(malformed_runner(format!(
            "file {name} local name contradicts the central directory"
        )));
    }

    if flags & ZIP_FLAG_DATA_DESCRIPTOR == 0 {
        let local_crc32 = u32::from_le_bytes([header[14], header[15], header[16], header[17]]);
        if local_crc32 != crc32 {
            return Err(malformed_runner(format!(
                "file {name} local CRC-32 {local_crc32:#010x} contradicts the central {crc32:#010x}"
            )));
        }

        let local_compressed = u32::from_le_bytes([header[18], header[19], header[20], header[21]]);
        let local_size = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
        // Zip64 moves the real values into an extra field and leaves 0xffffffff
        // here; the crate has already resolved them centrally, so a saturated
        // local field carries no information to compare against.
        if u64::from(local_size) != size && local_size != u32::MAX {
            return Err(malformed_runner(format!(
                "file {name} local size {local_size} contradicts the central {size}"
            )));
        }
        if local_compressed != local_size {
            return Err(malformed_runner(format!(
                "file {name} local compressed size {local_compressed} differs from size {local_size}"
            )));
        }
    }

    Ok(())
}

impl Archive {
    /// Open `bytes` as the ustar the runner registry ships. Every caller that
    /// reads a registry runner goes through here, so the loader and the
    /// out-of-band tools cannot disagree about the format.
    pub fn from_ustar_bytes(bytes: bytes::Bytes) -> errors::Result<Self> {
        let legacy = super::ustar::LegacyArchive::from_ustar(bytes)?;
        Ok(Self {
            data: legacy.data,
            total_size: legacy.total_size,
        })
    }

    /// Open `bytes` as a zip and read it. Entry offsets are resolved against the
    /// very buffer the central directory was read from, which is why the caller
    /// never gets to supply the two separately.
    pub fn from_zip_bytes(bytes: bytes::Bytes) -> errors::Result<Self> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone()))
            .map_err(|e| malformed_runner(format!("cannot open ZIP archive: {e}")))?;
        Self::from_zip(&mut zip, bytes)
    }

    /// Only for callers that already opened the zip to decide *whether* it is
    /// one; `bytes` must be the buffer `zip` reads from. Everyone else wants
    /// [`Archive::from_zip_bytes`].
    pub(super) fn from_zip<R: std::io::Read + std::io::Seek>(
        zip: &mut zip::ZipArchive<R>,
        bytes: bytes::Bytes,
    ) -> errors::Result<Self> {
        let mut res = BTreeMap::new();

        for i in 0..zip.len() {
            let file = zip
                .by_index(i)
                .map_err(|e| malformed_runner(format!("cannot read ZIP entry {i}: {e}")))?;

            if file.compression() != zip::CompressionMethod::Stored {
                return Err(malformed_runner(format!(
                    "unsupported compression method: {:?}",
                    file.compression()
                )));
            }

            check_local_header_agrees(
                bytes.as_ref(),
                file.name(),
                file.header_start(),
                file.crc32(),
                file.size(),
            )?;
            let is_directory = file.name().ends_with('/');
            validate_entry_name(file.name(), is_directory)?;

            let start_index = file.data_start();
            let compressed_size = file.compressed_size();
            // A stored entry is copied verbatim, so both sizes describe the same
            // bytes; disagreeing ones mean the archive lies about at least one.
            if compressed_size != file.size() {
                return Err(malformed_runner(format!(
                    "stored file {} compressed_size={} differs from size={}",
                    file.name(),
                    compressed_size,
                    file.size()
                )));
            }
            if is_directory {
                if file.size() != 0 || file.crc32() != 0 {
                    return Err(malformed_runner(format!(
                        "directory entry {} has size {} and CRC-32 {:#010x}",
                        file.name(),
                        file.size(),
                        file.crc32()
                    )));
                }
                continue;
            }
            let end_index = start_index.checked_add(compressed_size).ok_or_else(|| {
                malformed_runner(format!(
                    "file {} data_start={} compressed_size={} overflow",
                    file.name(),
                    start_index,
                    compressed_size
                ))
            })?;
            if end_index > usize_into_u64(bytes.len())
                || start_index > usize_into_u64(bytes.len())
                || end_index < start_index
            {
                return Err(malformed_runner(format!(
                    "file {} data_start={} compressed_size={} end_index={} bytes_len={}",
                    file.name(),
                    start_index,
                    compressed_size,
                    end_index,
                    bytes.len()
                )));
            }
            let buf = bytes.slice(start_index.into_int_comptime()..end_index.into_int_comptime());
            let actual_crc = crc32fast::hash(&buf);
            if actual_crc != file.crc32() {
                return Err(malformed_runner(format!(
                    "file {} CRC-32 {actual_crc:#010x} differs from declared {:#010x}",
                    file.name(),
                    file.crc32()
                )));
            }

            // Last entry of a repeated name wins. The zip crate already collapses
            // duplicates by decoded name, so this only matters if that changes.
            res.insert(String::from(file.name()), buf);
        }

        let res = Self {
            data: res,
            total_size: bytes.len().into_int_downcast_panicking(),
        };
        // The sum of entry lengths may exceed `total_size`: a crafted zip can point
        // several entries at overlapping data ranges. That is attacker input, not a bug.
        Ok(res)
    }

    pub fn from_file_and_runner(
        file: bytes::Bytes,
        version: bytes::Bytes,
        runner_comment: bytes::Bytes,
    ) -> Self {
        let total_size = file.len().into_int_downcast_panicking();

        Self {
            data: BTreeMap::from_iter([
                ("runner.json".into(), runner_comment),
                ("version".into(), version),
                ("file".into(), file),
            ]),
            total_size,
        }
    }
}
