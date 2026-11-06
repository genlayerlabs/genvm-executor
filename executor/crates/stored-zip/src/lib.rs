//! Read-only zip over an in-memory buffer, for archives of stored entries.
//!
//! The layout is held to `[local entries][central directory][end record]` with
//! nothing between the directory and the end record, and the end record's
//! comment running exactly to the end of the buffer. Zip64, multi-disk end
//! records, encryption, data descriptors and extra fields are rejected. CRC-32,
//! whether the entries tile the bytes before the directory, and every other
//! entry field are left to the caller.

use std::ops::Range;

pub const STORED: u16 = 0;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const END_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;

const LOCAL_LEN: usize = 30;
const CENTRAL_LEN: usize = 46;
const END_LEN: usize = 22;
const ZIP64_LOCATOR_LEN: usize = 20;

const FLAG_ENCRYPTED: u16 = 1 << 0;
const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;
/// Without it, a name is CP437.
#[cfg(feature = "write")]
const FLAG_UTF8: u16 = 1 << 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NoEndRecord,
    MultiDisk,
    Zip64,
    CountMismatch,
    DirectoryBounds,
    BadCentralEntry,
    TrailingDirectoryBytes,
    HeaderAfterDirectory,
    Encrypted,
    DataDescriptor,
    ExtraField,
    LocalHeaderBounds,
    LocalSignature,
    DataBounds,
    #[cfg(feature = "write")]
    TooLarge,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::NoEndRecord => "no end of central directory record",
            Error::MultiDisk => "multi-disk archives are not supported",
            Error::Zip64 => "zip64 is not supported",
            Error::CountMismatch => "entry counts disagree with the central directory",
            Error::DirectoryBounds => "central directory does not end at the end record",
            Error::BadCentralEntry => {
                "central directory entry is truncated or has a wrong signature"
            }
            Error::TrailingDirectoryBytes => "central directory has bytes past its last entry",
            Error::HeaderAfterDirectory => "local header offset is past the central directory",
            Error::Encrypted => "entry is encrypted",
            Error::DataDescriptor => "stored entry uses a data descriptor",
            Error::ExtraField => "entry has an extra field",
            Error::LocalHeaderBounds => "local header is out of bounds",
            Error::LocalSignature => "local header signature is wrong",
            Error::DataBounds => "entry data runs past the central directory",
            #[cfg(feature = "write")]
            Error::TooLarge => "archive needs zip64",
        })
    }
}

impl std::error::Error for Error {}

fn le<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    le(bytes, at).map(u16::from_le_bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    le(bytes, at).map(u32::from_le_bytes)
}

fn to_usize(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// One central directory record. Names are raw bytes: the encoding flag is not
/// interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub version_needed: u16,
    pub flags: u16,
    pub method: u16,
    pub time: u16,
    pub date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub size: u32,
    pub name: &'a [u8],
    pub extra: &'a [u8],
    pub comment: &'a [u8],
    pub disk_start: u16,
    pub internal_attributes: u16,
    pub external_attributes: u32,
    pub header_offset: u32,
}

/// A local header, with `data` spanning the central compressed size from where
/// the header ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalHeader<'a> {
    pub version_needed: u16,
    pub flags: u16,
    pub method: u16,
    pub time: u16,
    pub date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub size: u32,
    pub name: &'a [u8],
    pub data: Range<usize>,
}

fn parse_central(cd: &[u8]) -> Option<(Entry<'_>, &[u8])> {
    let (header, rest) = cd.split_at_checked(CENTRAL_LEN)?;
    if u32_at(header, 0)? != CENTRAL_SIG {
        return None;
    }
    let (name, rest) = rest.split_at_checked(u16_at(header, 28)?.into())?;
    let (extra, rest) = rest.split_at_checked(u16_at(header, 30)?.into())?;
    let (comment, rest) = rest.split_at_checked(u16_at(header, 32)?.into())?;
    let entry = Entry {
        version_needed: u16_at(header, 6)?,
        flags: u16_at(header, 8)?,
        method: u16_at(header, 10)?,
        time: u16_at(header, 12)?,
        date: u16_at(header, 14)?,
        crc32: u32_at(header, 16)?,
        compressed_size: u32_at(header, 20)?,
        size: u32_at(header, 24)?,
        name,
        extra,
        comment,
        disk_start: u16_at(header, 34)?,
        internal_attributes: u16_at(header, 36)?,
        external_attributes: u32_at(header, 38)?,
        header_offset: u32_at(header, 42)?,
    };
    Some((entry, rest))
}

/// The last end signature, which only counts if its comment length reaches
/// exactly to the end.
fn find_end(buf: &[u8]) -> Option<usize> {
    let last = buf.len().checked_sub(END_LEN)?;
    let first = last.saturating_sub(u16::MAX.into());
    let at = (first..=last)
        .rev()
        .find(|&at| u32_at(buf, at) == Some(END_SIG))?;
    (u16_at(buf, at + 20).map(usize::from) == Some(last - at)).then_some(at)
}

fn check_flags(flags: u16) -> Result<(), Error> {
    if flags & FLAG_ENCRYPTED != 0 {
        return Err(Error::Encrypted);
    }
    if flags & FLAG_DATA_DESCRIPTOR != 0 {
        return Err(Error::DataDescriptor);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct Archive<'a> {
    buf: &'a [u8],
    cd: &'a [u8],
    cd_start: usize,
    count: u16,
    comment: &'a [u8],
}

impl<'a> Archive<'a> {
    /// Locates the end record and walks the whole central directory.
    pub fn new(buf: &'a [u8]) -> Result<Self, Error> {
        let end_at = find_end(buf).ok_or(Error::NoEndRecord)?;
        let field16 = |at| u16_at(buf, end_at + at).ok_or(Error::NoEndRecord);
        let field32 = |at| u32_at(buf, end_at + at).ok_or(Error::NoEndRecord);

        if field16(4)? != 0 || field16(6)? != 0 {
            return Err(Error::MultiDisk);
        }
        let count = field16(10)?;
        if field16(8)? != count {
            return Err(Error::CountMismatch);
        }
        let cd_size = to_usize(field32(12)?);
        let cd_start = to_usize(field32(16)?);
        if cd_start.checked_add(cd_size) != Some(end_at) {
            let locator = end_at
                .checked_sub(ZIP64_LOCATOR_LEN)
                .and_then(|at| u32_at(buf, at));
            return Err(if locator == Some(ZIP64_LOCATOR_SIG) {
                Error::Zip64
            } else {
                Error::DirectoryBounds
            });
        }
        let cd = buf.get(cd_start..end_at).ok_or(Error::DirectoryBounds)?;
        if usize::from(count).saturating_mul(CENTRAL_LEN) > cd.len() {
            return Err(Error::CountMismatch);
        }

        let mut rest = cd;
        for _ in 0..count {
            let (entry, next) = parse_central(rest).ok_or(Error::BadCentralEntry)?;
            if to_usize(entry.header_offset) >= cd_start {
                return Err(Error::HeaderAfterDirectory);
            }
            rest = next;
        }
        if !rest.is_empty() {
            return Err(Error::TrailingDirectoryBytes);
        }

        Ok(Self {
            buf,
            cd,
            cd_start,
            count,
            comment: buf.get(end_at + END_LEN..).unwrap_or_default(),
        })
    }

    pub fn len(&self) -> usize {
        self.count.into()
    }

    /// Offset of the central directory, which is where the last entry must end.
    pub fn directory_start(&self) -> usize {
        self.cd_start
    }

    /// The end record's comment.
    pub fn comment(&self) -> &'a [u8] {
        self.comment
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Entries in directory order, duplicates included.
    pub fn entries(&self) -> Entries<'a> {
        Entries {
            rest: self.cd,
            remaining: self.count,
        }
    }

    /// Reads the local header of a stored `entry`, rejecting what would make
    /// the entry mean different things to different readers. Agreement of the
    /// two headers' fields is left to the caller.
    pub fn local_header(&self, entry: &Entry<'a>) -> Result<LocalHeader<'a>, Error> {
        check_flags(entry.flags)?;
        if !entry.extra.is_empty() {
            return Err(Error::ExtraField);
        }

        let before_cd = self.buf.get(..self.cd_start).unwrap_or_default();
        let start = to_usize(entry.header_offset);
        let header = start
            .checked_add(LOCAL_LEN)
            .and_then(|end| before_cd.get(start..end))
            .ok_or(Error::LocalHeaderBounds)?;
        let field16 = |at| u16_at(header, at).ok_or(Error::LocalHeaderBounds);
        let field32 = |at| u32_at(header, at).ok_or(Error::LocalHeaderBounds);
        if field32(0)? != LOCAL_SIG {
            return Err(Error::LocalSignature);
        }
        let flags = field16(6)?;
        check_flags(flags)?;

        let name_start = start + LOCAL_LEN;
        let extra_start = name_start
            .checked_add(field16(26)?.into())
            .ok_or(Error::LocalHeaderBounds)?;
        let data_start = extra_start
            .checked_add(field16(28)?.into())
            .ok_or(Error::LocalHeaderBounds)?;
        let name = before_cd
            .get(name_start..extra_start)
            .ok_or(Error::LocalHeaderBounds)?;
        if data_start != extra_start {
            return Err(Error::ExtraField);
        }

        let data_end = data_start
            .checked_add(to_usize(entry.compressed_size))
            .filter(|&end| end <= before_cd.len())
            .ok_or(Error::DataBounds)?;

        Ok(LocalHeader {
            version_needed: field16(4)?,
            flags,
            method: field16(8)?,
            time: field16(10)?,
            date: field16(12)?,
            crc32: field32(14)?,
            compressed_size: field32(18)?,
            size: field32(22)?,
            name,
            data: data_start..data_end,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Entries<'a> {
    rest: &'a [u8],
    remaining: u16,
}

impl<'a> Iterator for Entries<'a> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Entry<'a>> {
        self.remaining = self.remaining.checked_sub(1)?;
        let (entry, rest) = parse_central(self.rest)?;
        self.rest = rest;
        Some(entry)
    }
}

/// Writes entries verbatim under whatever method they claim, so tests can build
/// archives the reader must reject.
#[cfg(feature = "write")]
#[derive(Debug, Default)]
pub struct Writer {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

#[cfg(feature = "write")]
impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, name: &str, method: u16, data: &[u8]) -> Result<(), Error> {
        let name_len = u16::try_from(name.len()).map_err(|_| Error::TooLarge)?;
        let size = u32::try_from(data.len()).map_err(|_| Error::TooLarge)?;
        let offset = u32::try_from(self.out.len()).map_err(|_| Error::TooLarge)?;
        self.count = self.count.checked_add(1).ok_or(Error::TooLarge)?;
        let flags = if name.is_ascii() { 0 } else { FLAG_UTF8 };
        let crc32 = crc32fast::hash(data);

        // version needed, flags, method, time, date, crc, sizes, name length
        let mut shared = Vec::with_capacity(24);
        for half in [20, flags, method, 0, 0x21] {
            shared.extend_from_slice(&half.to_le_bytes());
        }
        for word in [crc32, size, size] {
            shared.extend_from_slice(&word.to_le_bytes());
        }
        shared.extend_from_slice(&name_len.to_le_bytes());

        self.out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        self.out.extend_from_slice(&shared);
        self.out.extend_from_slice(&0_u16.to_le_bytes());
        self.out.extend_from_slice(name.as_bytes());
        self.out.extend_from_slice(data);

        self.central.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
        self.central.extend_from_slice(&20_u16.to_le_bytes());
        self.central.extend_from_slice(&shared);
        // extra, comment, disk, internal attributes, external attributes
        self.central.extend_from_slice(&[0; 12]);
        self.central.extend_from_slice(&offset.to_le_bytes());
        self.central.extend_from_slice(name.as_bytes());
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<u8>, Error> {
        let cd_start = u32::try_from(self.out.len()).map_err(|_| Error::TooLarge)?;
        let cd_size = u32::try_from(self.central.len()).map_err(|_| Error::TooLarge)?;
        self.out.append(&mut self.central);
        self.out.extend_from_slice(&END_SIG.to_le_bytes());
        for half in [0, 0, self.count, self.count] {
            self.out.extend_from_slice(&half.to_le_bytes());
        }
        self.out.extend_from_slice(&cd_size.to_le_bytes());
        self.out.extend_from_slice(&cd_start.to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes());
        Ok(self.out)
    }
}
