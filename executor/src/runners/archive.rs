use std::collections::{BTreeMap, BTreeSet};

use genlayer_sdk::abi;
use genvm_common::internal_constants;

use crate::int_traits::*;
use crate::rt::errors;

#[derive(Clone)]
pub struct Archive {
    pub data: BTreeMap<String, bytes::Bytes>,
    pub total_size: u32,
    pub meta_size: u32,
}

use super::malformed_runner_error as malformed_runner;

const ZIP_VERSION_NEEDED: u16 = 20;
const DOS_ATTRIBUTES_MASK: u32 = 0xffff;
const UNIX_FILE_TYPE_MASK: u32 = 0o170000 << 16;
const UNIX_REGULAR_FILE: u32 = 0o100000 << 16;

fn validate_entry_name(name: &str) -> errors::Result<()> {
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

    if name
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(malformed_runner(format!(
            "entry name {name:?} contains an invalid path component"
        )));
    }

    Ok(())
}

/// Genvm follows the central directory, a streaming reader follows the local
/// header, and a third tool may reject the file outright. Runner ids are content
/// hashes, so an archive whose two copies disagree is consensus-visible --
/// require them to agree.
fn check_local_header_agrees(
    name: &str,
    central: &genvm_stored_zip::Entry<'_>,
    local: &genvm_stored_zip::LocalHeader<'_>,
) -> errors::Result<()> {
    if local.method != genvm_stored_zip::STORED {
        return Err(malformed_runner(format!(
            "file {name} local compression method {} contradicts the central directory",
            local.method
        )));
    }

    if local.version_needed != central.version_needed {
        return Err(malformed_runner(format!(
            "file {name} local version {} contradicts the central {}",
            local.version_needed, central.version_needed
        )));
    }

    if local.time != central.time {
        return Err(malformed_runner(format!(
            "file {name} local time {:#06x} contradicts the central {:#06x}",
            local.time, central.time
        )));
    }

    if local.date != central.date {
        return Err(malformed_runner(format!(
            "file {name} local date {:#06x} contradicts the central {:#06x}",
            local.date, central.date
        )));
    }

    if local.flags != central.flags {
        return Err(malformed_runner(format!(
            "file {name} local flags {:#06x} contradict the central {:#06x}",
            local.flags, central.flags
        )));
    }

    if local.name != central.name {
        return Err(malformed_runner(format!(
            "file {name} local name contradicts the central directory"
        )));
    }

    if local.crc32 != central.crc32 {
        return Err(malformed_runner(format!(
            "file {name} local CRC-32 {:#010x} contradicts the central {:#010x}",
            local.crc32, central.crc32
        )));
    }

    if local.size != central.size {
        return Err(malformed_runner(format!(
            "file {name} local size {} contradicts the central {}",
            local.size, central.size
        )));
    }
    if local.compressed_size != local.size {
        return Err(malformed_runner(format!(
            "file {name} local compressed size {} differs from size {}",
            local.compressed_size, local.size
        )));
    }

    Ok(())
}

/// Version made by and permission bits stay free. File type bits may be absent:
/// `zipfile.writestr` sets only the permissions.
fn check_entry_fields(name: &str, entry: &genvm_stored_zip::Entry<'_>) -> errors::Result<()> {
    if entry.version_needed != ZIP_VERSION_NEEDED {
        return Err(malformed_runner(format!(
            "file {name} needs version {}",
            entry.version_needed
        )));
    }
    if entry.disk_start != 0 {
        return Err(malformed_runner(format!(
            "file {name} starts on disk {}",
            entry.disk_start
        )));
    }
    if entry.internal_attributes != 0 {
        return Err(malformed_runner(format!(
            "file {name} has internal attributes {:#06x}",
            entry.internal_attributes
        )));
    }

    let attributes = entry.external_attributes;
    let file_type = attributes & UNIX_FILE_TYPE_MASK;
    if attributes & DOS_ATTRIBUTES_MASK != 0 || (file_type != 0 && file_type != UNIX_REGULAR_FILE) {
        return Err(malformed_runner(format!(
            "file {name} has external attributes {attributes:#010x}"
        )));
    }

    Ok(())
}

impl Archive {
    /// Read the zip of an installed runner. Its bytes are the executor's own, so
    /// only exceeding `max_meta_size` is a VM error.
    pub fn from_zip_bytes(bytes: bytes::Bytes, max_meta_size: u32) -> errors::Result<Self> {
        let oom = abi::consts::VmError::out_of().memory().val();
        match Self::try_from_zip(&bytes, max_meta_size) {
            Ok(Ok(archive)) => Ok(archive),
            Ok(Err(e)) if matches!(&e.kind, errors::ErrorKind::Vm(code) if *code == oom) => Err(e),
            Ok(Err(e)) => Err(errors::internal!(
                "installed runner archive is malformed: {e}"
            )),
            Err(e) => Err(errors::internal!("cannot open ZIP archive: {e}")),
        }
    }

    /// The outer error means `bytes` is not a zip at all, the inner one that it
    /// is a malformed one.
    pub(super) fn try_from_zip(
        bytes: &bytes::Bytes,
        max_meta_size: u32,
    ) -> Result<errors::Result<Self>, genvm_stored_zip::Error> {
        let zip = genvm_stored_zip::Archive::new(bytes)?;
        Ok(Self::from_zip(&zip, bytes, max_meta_size))
    }

    /// `bytes` must be the buffer `zip` was opened on.
    fn from_zip(
        zip: &genvm_stored_zip::Archive<'_>,
        bytes: &bytes::Bytes,
        max_meta_size: u32,
    ) -> errors::Result<Self> {
        if !zip.comment().is_empty() {
            return Err(malformed_runner("ZIP archive has a comment"));
        }

        // The whole directory is charged before any entry is checked, so an
        // archive over budget is out of memory whatever is wrong with its entries.
        let meta_size = zip.entries().fold(0u32, |acc, entry| {
            acc.saturating_add(entry.name.len().try_into().unwrap_or(u32::MAX))
                .saturating_add(internal_constants::memory_limiter_consts::ZIP_FILE_COST)
        });
        if meta_size > max_meta_size {
            return Err(errors::Error::vm(
                abi::consts::VmError::out_of().memory().val(),
            ));
        }

        let mut names = BTreeSet::new();
        let mut files = Vec::new();
        // Entries tile everything before the directory in directory order, so a
        // streaming reader sees the same files and the bytes hashed below are
        // bounded by the archive size.
        let mut next_offset = 0usize;

        for (i, entry) in zip.entries().enumerate() {
            let name = std::str::from_utf8(entry.name)
                .ok()
                .filter(|name| name.bytes().all(|b| matches!(b, b' '..=b'~')))
                .ok_or_else(|| {
                    malformed_runner(format!(
                        "ZIP entry {i} name \"{}\" is not printable ASCII",
                        entry.name.escape_ascii()
                    ))
                })?;

            if entry.method != genvm_stored_zip::STORED {
                return Err(malformed_runner(format!(
                    "unsupported compression method: {}",
                    entry.method
                )));
            }

            let offset: usize = entry.header_offset.into_int_comptime();
            if offset != next_offset {
                return Err(malformed_runner(format!(
                    "file {name} starts at {offset}, not at {next_offset}"
                )));
            }

            let local = zip
                .local_header(&entry)
                .map_err(|e| malformed_runner(format!("file {name}: {e}")))?;
            check_local_header_agrees(name, &entry, &local)?;
            check_entry_fields(name, &entry)?;
            if entry.flags != 0 {
                return Err(malformed_runner(format!(
                    "file {name} has unsupported flags {:#06x}",
                    entry.flags
                )));
            }
            if !entry.comment.is_empty() {
                return Err(malformed_runner(format!("file {name} has a comment")));
            }

            // A stored entry is copied verbatim, so both sizes describe the same
            // bytes; disagreeing ones mean the archive lies about at least one.
            if entry.compressed_size != entry.size {
                return Err(malformed_runner(format!(
                    "stored file {name} compressed_size={} differs from size={}",
                    entry.compressed_size, entry.size
                )));
            }
            next_offset = local.data.end;

            if name.ends_with('/') {
                return Err(malformed_runner(format!(
                    "directory entry {name} is not allowed"
                )));
            }

            validate_entry_name(name)?;
            if !names.insert(name) {
                return Err(malformed_runner(format!("file {name} is repeated")));
            }
            files.push((name, local.data, entry.crc32));
        }

        if next_offset != zip.directory_start() {
            return Err(malformed_runner(format!(
                "ZIP has bytes from {next_offset} before the central directory at {}",
                zip.directory_start()
            )));
        }

        let mut directory = String::new();
        for name in &names {
            directory.clear();
            directory.push_str(name);
            directory.push('/');
            if let Some(child) = names
                .range(directory.as_str()..)
                .next()
                .filter(|child| child.starts_with(directory.as_str()))
            {
                return Err(malformed_runner(format!(
                    "file {name} is also a directory of {child}"
                )));
            }
        }

        let mut res = BTreeMap::new();
        for (name, data, crc32) in files {
            let buf = bytes.slice(data);
            let actual_crc = crc32fast::hash(&buf);
            if actual_crc != crc32 {
                return Err(malformed_runner(format!(
                    "file {name} CRC-32 {actual_crc:#010x} differs from declared {crc32:#010x}"
                )));
            }
            res.insert(String::from(name), buf);
        }

        Ok(Self {
            data: res,
            total_size: bytes.len().into_int_downcast_panicking(),
            meta_size,
        })
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
            meta_size: 0,
        }
    }
}
