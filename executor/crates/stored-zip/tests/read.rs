use genvm_stored_zip::{Archive, Error, Writer, STORED};

fn archive_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = Writer::new();
    for (name, data) in entries {
        writer.add(name, STORED, data).unwrap();
    }
    writer.finish().unwrap()
}

fn sample() -> Vec<u8> {
    archive_of(&[
        ("runner.json", b"{}"),
        ("dir/", b""),
        ("dir/file", b"file contents"),
    ])
}

fn end_at(archive: &[u8]) -> usize {
    archive.len() - 22
}

fn cd_start(archive: &[u8]) -> usize {
    let at = end_at(archive) + 16;
    u32::from_le_bytes(archive[at..at + 4].try_into().unwrap()) as usize
}

fn put_u16(archive: &mut [u8], at: usize, value: u16) {
    archive[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(archive: &mut [u8], at: usize, value: u32) {
    archive[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn open_err(archive: &[u8]) -> Error {
    Archive::new(archive).unwrap_err()
}

fn first_local_err(archive: &[u8]) -> Error {
    let zip = Archive::new(archive).unwrap();
    let entry = zip.entries().next().unwrap();
    zip.local_header(&entry).unwrap_err()
}

/// Names and data of every entry, in directory order.
type Listing<'a> = Vec<(&'a [u8], &'a [u8])>;

/// Reads every entry the way the executor does, so a malformed input exercises
/// both halves of the parser.
fn read_all(archive: &[u8]) -> Result<Listing<'_>, Error> {
    let zip = Archive::new(archive)?;
    zip.entries()
        .map(|entry| {
            let local = zip.local_header(&entry)?;
            Ok((entry.name, &archive[local.data]))
        })
        .collect()
}

// Well-formed archives

#[test]
fn written_entries_read_back() {
    let archive = sample();
    let actual = read_all(&archive).unwrap();
    let expected: Listing = vec![
        (b"runner.json", b"{}"),
        (b"dir/", b""),
        (b"dir/file", b"file contents"),
    ];
    assert_eq!(actual, expected);
}

#[test]
fn central_and_local_fields_agree() {
    let archive = sample();
    let zip = Archive::new(&archive).unwrap();
    assert_eq!(zip.len(), 3);
    for entry in zip.entries() {
        let local = zip.local_header(&entry).unwrap();
        assert_eq!(local.name, entry.name);
        assert_eq!(local.crc32, entry.crc32);
        assert_eq!(local.size, entry.size);
        assert_eq!(local.compressed_size, entry.compressed_size);
        assert_eq!(local.method, STORED);
        assert_eq!(crc32fast::hash(&archive[local.data]), entry.crc32);
    }
}

#[test]
fn empty_archive_is_an_end_record_alone() {
    let archive = Writer::new().finish().unwrap();
    assert_eq!(archive.len(), 22);
    assert!(Archive::new(&archive).unwrap().is_empty());
}

#[test]
fn duplicate_names_are_all_listed() {
    let archive = archive_of(&[("a", b"first"), ("a", b"second")]);
    let expected: Listing = vec![(b"a", b"first"), (b"a", b"second")];
    assert_eq!(read_all(&archive).unwrap(), expected);
}

#[test]
fn comment_of_the_declared_length_is_accepted() {
    let mut archive = sample();
    let end = end_at(&archive);
    put_u16(&mut archive, end + 20, 5);
    archive.extend_from_slice(b"hello");
    assert_eq!(read_all(&archive).unwrap().len(), 3);
}

/// Readers that take the last signature would read the fake record, so the
/// archive is ambiguous rather than skipped past.
#[test]
fn end_signature_inside_the_comment_is_rejected() {
    let mut archive = sample();
    let end = end_at(&archive);
    let comment = b"PK\x05\x06 not an end record";
    put_u16(&mut archive, end + 20, comment.len() as u16);
    archive.extend_from_slice(comment);
    assert_eq!(open_err(&archive), Error::NoEndRecord);
}

#[test]
fn other_methods_are_reported_not_rejected() {
    let mut writer = Writer::new();
    writer.add("payload", 8, b"not really deflated").unwrap();
    let archive = writer.finish().unwrap();
    let zip = Archive::new(&archive).unwrap();
    assert_eq!(zip.entries().next().unwrap().method, 8);
}

// Never panics

#[test]
fn every_truncation_is_rejected() {
    let archive = sample();
    for len in 0..archive.len() {
        assert!(
            Archive::new(&archive[..len]).is_err(),
            "prefix of {len} bytes accepted"
        );
    }
    for start in 1..archive.len() {
        assert!(
            Archive::new(&archive[start..]).is_err(),
            "suffix from {start} accepted"
        );
    }
}

#[test]
fn every_single_byte_corruption_is_survived() {
    let archive = sample();
    for at in 0..archive.len() {
        for value in [0x00, 0x01, 0x7f, 0x80, 0xff] {
            let mut corrupted = archive.clone();
            corrupted[at] = value;
            let _ = read_all(&corrupted);
        }
    }
}

// End record

#[test]
fn wrong_end_signature_is_rejected() {
    let mut archive = sample();
    let end = end_at(&archive);
    archive[end] ^= 1;
    assert_eq!(open_err(&archive), Error::NoEndRecord);
}

#[test]
fn comment_length_past_the_end_is_rejected() {
    let mut archive = sample();
    let end = end_at(&archive);
    put_u16(&mut archive, end + 20, 1);
    assert_eq!(open_err(&archive), Error::NoEndRecord);
}

#[test]
fn bytes_after_the_comment_are_rejected() {
    let mut archive = sample();
    archive.push(0);
    assert_eq!(open_err(&archive), Error::NoEndRecord);
}

#[test]
fn multi_disk_is_rejected() {
    for field in [4, 6] {
        let mut archive = sample();
        let end = end_at(&archive);
        put_u16(&mut archive, end + field, 1);
        assert_eq!(open_err(&archive), Error::MultiDisk, "field at {field}");
    }
}

#[test]
fn disagreeing_counts_are_rejected() {
    let mut archive = sample();
    let end = end_at(&archive);
    put_u16(&mut archive, end + 8, 2);
    assert_eq!(open_err(&archive), Error::CountMismatch);
}

#[test]
fn count_beyond_the_directory_is_rejected() {
    for count in [4, u16::MAX] {
        let mut archive = sample();
        let end = end_at(&archive);
        put_u16(&mut archive, end + 8, count);
        put_u16(&mut archive, end + 10, count);
        let err = open_err(&archive);
        assert!(
            matches!(err, Error::CountMismatch | Error::BadCentralEntry),
            "count {count}: {err:?}"
        );
    }
}

#[test]
fn count_short_of_the_directory_is_rejected() {
    let mut archive = sample();
    let end = end_at(&archive);
    put_u16(&mut archive, end + 8, 2);
    put_u16(&mut archive, end + 10, 2);
    assert_eq!(open_err(&archive), Error::TrailingDirectoryBytes);
}

#[test]
fn directory_not_ending_at_the_end_record_is_rejected() {
    for (field, delta) in [(12, 1), (12, -1), (16, 1), (16, -1)] {
        let mut archive = sample();
        let at = end_at(&archive) + field;
        let value = u32::from_le_bytes(archive[at..at + 4].try_into().unwrap());
        put_u32(&mut archive, at, value.wrapping_add_signed(delta));
        let err = open_err(&archive);
        assert!(
            matches!(err, Error::DirectoryBounds | Error::BadCentralEntry),
            "field {field} {delta:+}: {err:?}"
        );
    }
}

#[test]
fn directory_offset_overflow_is_rejected() {
    let mut archive = sample();
    let end = end_at(&archive);
    put_u32(&mut archive, end + 12, u32::MAX);
    put_u32(&mut archive, end + 16, u32::MAX);
    assert_eq!(open_err(&archive), Error::DirectoryBounds);
}

#[test]
fn zip64_locator_is_rejected() {
    let mut archive = sample();
    let end = end_at(&archive);
    let mut locator = b"PK\x06\x07".to_vec();
    locator.resize(20, 0);
    archive.splice(end..end, locator);
    assert_eq!(open_err(&archive), Error::Zip64);
}

// Central directory

#[test]
fn wrong_central_signature_is_rejected() {
    let mut archive = sample();
    let cd = cd_start(&archive);
    archive[cd] ^= 1;
    assert_eq!(open_err(&archive), Error::BadCentralEntry);
}

#[test]
fn central_name_length_past_the_directory_is_rejected() {
    let mut archive = sample();
    let cd = cd_start(&archive);
    put_u16(&mut archive, cd + 28, u16::MAX);
    assert_eq!(open_err(&archive), Error::BadCentralEntry);
}

#[test]
fn local_header_offset_past_the_directory_is_rejected() {
    let mut archive = sample();
    let cd = cd_start(&archive);
    put_u32(&mut archive, cd + 42, cd as u32);
    assert_eq!(open_err(&archive), Error::HeaderAfterDirectory);
}

// Local headers

#[test]
fn wrong_local_signature_is_rejected() {
    let mut archive = sample();
    archive[0] ^= 1;
    assert_eq!(first_local_err(&archive), Error::LocalSignature);
}

#[test]
fn local_header_running_into_the_directory_is_rejected() {
    let mut archive = sample();
    let cd = cd_start(&archive);
    put_u32(&mut archive, cd + 42, cd as u32 - 1);
    assert_eq!(first_local_err(&archive), Error::LocalHeaderBounds);
}

#[test]
fn local_name_running_into_the_directory_is_rejected() {
    let mut archive = sample();
    put_u16(&mut archive, 26, u16::MAX);
    assert_eq!(first_local_err(&archive), Error::LocalHeaderBounds);
}

#[test]
fn data_running_into_the_directory_is_rejected() {
    let mut archive = archive_of(&[("payload", b"contents")]);
    let cd = cd_start(&archive);
    put_u32(&mut archive, cd + 20, 9);
    assert_eq!(first_local_err(&archive), Error::DataBounds);
}

#[test]
fn huge_compressed_size_is_rejected() {
    let mut archive = sample();
    let cd = cd_start(&archive);
    put_u32(&mut archive, cd + 20, u32::MAX);
    assert_eq!(first_local_err(&archive), Error::DataBounds);
}

#[test]
fn encryption_is_rejected_in_either_header() {
    for at in [6, cd_start(&sample()) + 8] {
        let mut archive = sample();
        put_u16(&mut archive, at, 1);
        assert_eq!(first_local_err(&archive), Error::Encrypted, "flags at {at}");
    }
}

#[test]
fn data_descriptor_is_rejected_in_either_header() {
    for at in [6, cd_start(&sample()) + 8] {
        let mut archive = sample();
        put_u16(&mut archive, at, 1 << 3);
        assert_eq!(
            first_local_err(&archive),
            Error::DataDescriptor,
            "flags at {at}"
        );
    }
}

/// Builds a single-entry archive whose central record carries `extra`, keeping
/// every offset consistent.
fn archive_with_central_extra(extra: &[u8]) -> Vec<u8> {
    let mut archive = archive_of(&[("payload", b"contents")]);
    let cd = cd_start(&archive);
    put_u16(&mut archive, cd + 30, extra.len() as u16);
    let name_end = cd + 46 + "payload".len();
    archive.splice(name_end..name_end, extra.iter().copied());
    let end = end_at(&archive);
    let cd_size = (end - cd) as u32;
    put_u32(&mut archive, end + 12, cd_size);
    archive
}

#[test]
fn central_extra_fields_are_rejected() {
    let mut zip64 = vec![0x01, 0x00, 24, 0];
    zip64.resize(28, 0);
    let extras: [&[u8]; 4] = [
        &zip64,
        &[0x75, 0x70, 2, 0, 1, 0],
        &[0x55, 0x54, 1, 0, 0],
        &[0xff],
    ];
    for extra in extras {
        let archive = archive_with_central_extra(extra);
        assert_eq!(first_local_err(&archive), Error::ExtraField, "{extra:?}");
    }
}

#[test]
fn local_extra_field_is_rejected() {
    let mut archive = archive_of(&[("payload", b"contents")]);
    put_u16(&mut archive, 28, 2);
    assert_eq!(first_local_err(&archive), Error::ExtraField);
}
