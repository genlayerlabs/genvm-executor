const DEFLATED: u16 = 8;

/// The writer copies `contents` verbatim whatever `method` says: genvm never
/// decompresses, so a non-stored entry only has to claim its method.
fn zip_with(contents: &[u8], method: u16) -> Vec<u8> {
    let mut writer = genvm_stored_zip::Writer::new();
    writer.add("payload", method, contents).unwrap();
    writer.finish().unwrap()
}

fn stored_zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = genvm_stored_zip::Writer::new();
    for (name, contents) in entries {
        writer
            .add(name, genvm_stored_zip::STORED, contents)
            .unwrap();
    }
    writer.finish().unwrap()
}

fn zip_with_duplicate_names() -> Vec<u8> {
    stored_zip_with_entries(&[("payload1", b"first"), ("payload1", b"second")])
}

/// A minimal single-file ustar archive, the format runners were packaged in
/// before ZIP became the only one.
fn ustar_with(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut archive = vec![0_u8; 512];
    archive[..name.len()].copy_from_slice(name.as_bytes());
    let size = format!("{:011o}\0", contents.len());
    archive[124..136].copy_from_slice(size.as_bytes());
    archive[156] = b'0';
    archive[257..265].copy_from_slice(b"ustar\x0000");
    archive.extend_from_slice(contents);
    archive.resize(archive.len().next_multiple_of(512), 0);
    archive.resize(archive.len() + 1024, 0);
    archive
}

fn central_header_start(archive: &[u8]) -> usize {
    archive
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .expect("archive has a central directory")
}

fn local_data_start(archive: &[u8]) -> usize {
    let name_len = usize::from(u16::from_le_bytes([archive[26], archive[27]]));
    let extra_len = usize::from(u16::from_le_bytes([archive[28], archive[29]]));
    30 + name_len + extra_len
}

fn error_of(archive: Vec<u8>) -> genvm::rt::errors::Error {
    match genvm::runners::parse(archive.into(), u32::MAX) {
        Ok(parsed) => panic!("archive was accepted with entries {:?}", parsed.data.keys()),
        Err(err) => err,
    }
}

fn assert_malformed_runner(actual: &genvm::rt::errors::Error) {
    let expected = genlayer_sdk::abi::consts::VmError::invalid_contract()
        .runner()
        .malformed();
    assert!(
        matches!(&actual.kind, genvm::rt::errors::ErrorKind::Vm(code) if code == &expected),
        "unexpected error: {actual}"
    );
}

#[test]
fn duplicate_zip_entry_names_are_rejected() {
    let actual = error_of(zip_with_duplicate_names());
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("repeated"),
        "unexpected error: {actual}"
    );
}

#[test]
fn directory_entries_are_rejected() {
    for name in ["dir/", "dir/sub/", "/", "/abs/", "../evil/", "a\\b/"] {
        let actual = error_of(stored_zip_with_entries(&[(name, b"")]));
        assert_malformed_runner(&actual);
        assert!(
            actual.to_string().contains("directory entry"),
            "unexpected error for {name:?}: {actual}"
        );
    }
}

fn assert_out_of_memory(actual: &genvm::rt::errors::Error) {
    let expected = genlayer_sdk::abi::consts::VmError::out_of().memory().val();
    assert!(
        matches!(&actual.kind, genvm::rt::errors::ErrorKind::Vm(code) if code == &expected),
        "unexpected error: {actual}"
    );
}

#[test]
fn directory_entries_are_charged() {
    let cost = genvm_common::internal_constants::memory_limiter_consts::ZIP_FILE_COST;
    let archive = stored_zip_with_entries(&[("dir/", b"")]);
    match genvm::runners::parse(archive.clone().into(), cost + 4) {
        Ok(_) => panic!("archive was accepted"),
        Err(err) => assert_malformed_runner(&err),
    }
    match genvm::runners::parse(archive.into(), cost + 3) {
        Ok(_) => panic!("archive was accepted"),
        Err(err) => assert_out_of_memory(&err),
    }
}

#[test]
fn charge_precedes_entry_checks() {
    let cost = genvm_common::internal_constants::memory_limiter_consts::ZIP_FILE_COST;
    let mut archive = stored_zip_with_entries(&[("a", b"first"), ("b", b"second")]);
    archive[0] ^= 1;
    match genvm::runners::parse(archive.into(), 2 * cost + 1) {
        Ok(_) => panic!("archive was accepted"),
        Err(err) => assert_out_of_memory(&err),
    }
}

#[test]
fn file_that_is_also_a_directory_is_rejected() {
    let file: &[u8] = b"file";
    let cases: [&[(&str, &[u8])]; 4] = [
        &[("a", file), ("a/b", file)],
        &[("a/b/c", file), ("a/b", file)],
        &[("a/b/c", file), ("a", file)],
        &[("a", file), ("a!b", file), ("a/b", file)],
    ];
    for entries in cases {
        let actual = error_of(stored_zip_with_entries(entries));
        assert_malformed_runner(&actual);
        assert!(
            actual.to_string().contains("also a directory"),
            "unexpected error for {entries:?}: {actual}"
        );
    }
}

#[test]
fn file_name_prefixes_are_not_directories() {
    let archive = stored_zip_with_entries(&[
        ("a", b"file"),
        ("a!b", b"file"),
        ("ab", b"file"),
        ("a.b/c", b"file"),
    ]);
    let actual = genvm::runners::parse(archive.into(), u32::MAX).unwrap();
    assert_eq!(
        actual.data.len(),
        4,
        "unexpected entries: {:?}",
        actual.data.keys()
    );
}

#[test]
fn archive_comment_is_rejected() {
    let mut archive = zip_with(b"contents", genvm_stored_zip::STORED);
    let comment_len = archive.len() - 2;
    archive[comment_len..].copy_from_slice(&4_u16.to_le_bytes());
    archive.extend_from_slice(b"note");

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("archive has a comment"),
        "unexpected error: {actual}"
    );
}

#[test]
fn entry_comment_is_rejected() {
    let mut archive = zip_with(b"contents", genvm_stored_zip::STORED);
    let cd = central_header_start(&archive);
    archive[cd + 32..cd + 34].copy_from_slice(&4_u16.to_le_bytes());
    let end = archive.len() - 22;
    archive.splice(end..end, *b"note");
    let cd_size = u32::from_le_bytes(archive[end + 4 + 12..end + 4 + 16].try_into().unwrap()) + 4;
    archive[end + 4 + 12..end + 4 + 16].copy_from_slice(&cd_size.to_le_bytes());

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("payload has a comment"),
        "unexpected error: {actual}"
    );
}

/// `outer` stores a whole local record of `inner` as its contents, and the
/// central directory lists both, so `inner` lies inside `outer`.
fn zip_with_nested_entries() -> Vec<u8> {
    const END_LEN: usize = 22;

    let inner = stored_zip_with_entries(&[("inner", b"contents")]);
    let inner_cd = central_header_start(&inner);
    let mut inner_central = inner[inner_cd..inner.len() - END_LEN].to_vec();

    let mut archive = stored_zip_with_entries(&[("outer", &inner[..inner_cd])]);
    let cd_start = central_header_start(&archive);
    let inner_offset = u32::try_from(local_data_start(&archive)).unwrap();
    inner_central[42..46].copy_from_slice(&inner_offset.to_le_bytes());

    let end = archive.len() - END_LEN;
    archive.splice(end..end, inner_central.iter().copied());
    let end = archive.len() - END_LEN;
    archive[end + 8..end + 10].copy_from_slice(&2_u16.to_le_bytes());
    archive[end + 10..end + 12].copy_from_slice(&2_u16.to_le_bytes());
    let cd_size = u32::try_from(end - cd_start).unwrap();
    archive[end + 12..end + 16].copy_from_slice(&cd_size.to_le_bytes());
    archive
}

#[test]
fn overlapping_zip_entries_are_rejected() {
    let actual = error_of(zip_with_nested_entries());
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("starts at"),
        "unexpected error: {actual}"
    );
}

// -- Entries tile everything before the central directory ----------------

fn u32_at(archive: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(archive[at..at + 4].try_into().unwrap())
}

fn put_u32(archive: &mut [u8], at: usize, value: u32) {
    archive[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// Start of each central record, assuming no extra fields or comments.
fn central_records(archive: &[u8]) -> Vec<usize> {
    let end = archive.len() - 22;
    let mut at = central_header_start(archive);
    let mut records = Vec::new();
    while at < end {
        records.push(at);
        at += 46 + usize::from(u16::from_le_bytes([archive[at + 28], archive[at + 29]]));
    }
    records
}

/// Inserts `gap` at `at`, moving every record behind it, so only the gap itself
/// is wrong with the result.
fn insert_gap(archive: &mut Vec<u8>, at: usize, gap: &[u8]) {
    let shift = u32::try_from(gap.len()).unwrap();
    for record in central_records(archive) {
        let offset = u32_at(archive, record + 42);
        if usize::try_from(offset).unwrap() >= at {
            put_u32(archive, record + 42, offset + shift);
        }
    }
    let cd_start_at = archive.len() - 22 + 16;
    let cd_start = u32_at(archive, cd_start_at);
    put_u32(archive, cd_start_at, cd_start + shift);
    archive.splice(at..at, gap.iter().copied());
}

fn two_entries() -> Vec<u8> {
    stored_zip_with_entries(&[("a", b"first"), ("b", b"other")])
}

fn second_entry_offset(archive: &[u8]) -> usize {
    let second = central_records(archive)[1];
    usize::try_from(u32_at(archive, second + 42)).unwrap()
}

fn assert_gap_rejected(archive: Vec<u8>, expected: &str) {
    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains(expected),
        "unexpected error: {actual}"
    );
}

#[test]
fn entries_without_gaps_are_accepted() {
    let actual = genvm::runners::parse(two_entries().into(), u32::MAX).unwrap();
    assert_eq!(actual.data.len(), 2);
}

#[test]
fn bytes_before_the_first_entry_are_rejected() {
    for gap in [&b"junk"[..], &[0; 4]] {
        let mut archive = two_entries();
        insert_gap(&mut archive, 0, gap);
        assert_gap_rejected(archive, "starts at 4");
    }
}

#[test]
fn bytes_between_entries_are_rejected() {
    for gap in [&b"junk"[..], &[0; 4]] {
        let mut archive = two_entries();
        let at = second_entry_offset(&archive);
        insert_gap(&mut archive, at, gap);
        assert_gap_rejected(archive, "starts at");
    }
}

#[test]
fn bytes_before_the_central_directory_are_rejected() {
    for gap in [&b"junk"[..], &[0; 4]] {
        let mut archive = two_entries();
        let at = central_header_start(&archive);
        insert_gap(&mut archive, at, gap);
        assert_gap_rejected(archive, "before the central directory");
    }
}

/// A streaming reader would see a file the central directory does not list.
#[test]
fn local_record_hidden_in_a_gap_is_rejected() {
    let hidden = stored_zip_with_entries(&[("hidden", b"contents")]);
    let hidden = &hidden[..central_header_start(&hidden)];

    let mut archive = two_entries();
    let at = second_entry_offset(&archive);
    insert_gap(&mut archive, at, hidden);
    assert_gap_rejected(archive, "starts at");

    let mut archive = two_entries();
    let at = central_header_start(&archive);
    insert_gap(&mut archive, at, hidden);
    assert_gap_rejected(archive, "before the central directory");
}

#[test]
fn bytes_before_an_empty_directory_are_rejected() {
    let mut archive = b"junk".to_vec();
    archive.extend(genvm_stored_zip::Writer::new().finish().unwrap());
    put_u32(&mut archive, 4 + 16, 4);
    assert_gap_rejected(archive, "before the central directory");
}

#[test]
fn directory_order_must_match_stream_order() {
    let mut archive = two_entries();
    let records = central_records(&archive);
    let len = records[1] - records[0];
    let (first, second) = archive[records[0]..].split_at_mut(len);
    first.swap_with_slice(&mut second[..len]);
    assert_gap_rejected(archive, "starts at");
}

#[test]
fn entry_repeated_in_the_directory_is_rejected() {
    let mut archive = two_entries();
    let records = central_records(&archive);
    let first = u32_at(&archive, records[0] + 42);
    put_u32(&mut archive, records[1] + 42, first);
    assert_gap_rejected(archive, "starts at");
}

#[test]
fn non_printable_ascii_names_are_rejected() {
    for name in ["é", "dir/é", "a\0b", "a\tb", "a\x7fb", "dir\x01/"] {
        let actual = error_of(stored_zip_with_entries(&[(name, b"")]));
        assert_malformed_runner(&actual);
        assert!(
            actual.to_string().contains("not printable ASCII"),
            "unexpected error for {name:?}: {actual}"
        );
    }
}

fn put_flags(archive: &mut [u8], local: u16, central: u16) {
    let cd = central_header_start(archive);
    archive[6..8].copy_from_slice(&local.to_le_bytes());
    archive[cd + 8..cd + 10].copy_from_slice(&central.to_le_bytes());
}

#[test]
fn local_flags_cannot_contradict_the_central_directory() {
    let mut archive = zip_with(b"contents", genvm_stored_zip::STORED);
    put_flags(&mut archive, 1 << 11, 0);

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("local flags"),
        "unexpected error: {actual}"
    );
}

#[test]
fn unsupported_flags_are_rejected() {
    for flag in [1 << 1, 1 << 6, 1 << 11, 1 << 13, 1 << 15] {
        let mut archive = zip_with(b"contents", genvm_stored_zip::STORED);
        put_flags(&mut archive, flag, flag);

        let actual = error_of(archive);
        assert_malformed_runner(&actual);
        assert!(
            actual.to_string().contains("unsupported flags"),
            "unexpected error for {flag:#06x}: {actual}"
        );
    }
}

/// ZIP is the only archive format; a ustar runner must not resurrect the
/// deleted parser by falling through to one of the other layouts either.
#[test]
fn ustar_archives_are_rejected() {
    let actual = error_of(ustar_with("runner.json", b"{}"));
    assert!(!actual.to_string().is_empty(), "unexpected error: {actual}");
}

#[test]
fn stored_entry_with_wrong_crc_is_rejected() {
    let mut archive = zip_with(b"crc payload", genvm_stored_zip::STORED);
    let data_start = local_data_start(&archive);
    archive[data_start] ^= 1;

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("CRC"),
        "unexpected error: {actual}"
    );
}

#[test]
fn invalid_zip_file_names_are_rejected() {
    for name in [
        "",
        "/payload",
        "./payload",
        "dir/../payload",
        "dir//payload",
        "dir\\payload",
    ] {
        let actual = error_of(stored_zip_with_entries(&[(name, b"contents")]));
        assert_malformed_runner(&actual);
        assert!(
            actual.to_string().contains("entry name"),
            "unexpected error for {name:?}: {actual}"
        );
    }
}

/// A `Stored` entry is copied verbatim, so its two sizes describe the same
/// bytes. Disagreeing sizes mean the archive lies about at least one of them,
/// and the parser must not pick a winner silently.
///
/// The local header is moved in step with the central one, so the local/central
/// agreement check passes and this exercises the central rule on its own.
#[test]
fn stored_entry_with_mismatched_sizes_is_rejected() {
    let contents = b"stored payload";
    let mut archive = zip_with(contents, genvm_stored_zip::STORED);
    let claimed = (contents.len() as u32 + 1).to_le_bytes();

    let uncompressed_size = central_header_start(&archive) + 24;
    archive[uncompressed_size..uncompressed_size + 4].copy_from_slice(&claimed);
    archive[18..22].copy_from_slice(&claimed);
    archive[22..26].copy_from_slice(&claimed);

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("compressed_size="),
        "unexpected error: {actual}"
    );
}

/// Every other tool decompresses the local entry; genvm only ever copies it. If
/// the central directory may claim `Stored` for a deflated entry, the same
/// archive publishes one file and executes another.
#[test]
fn deflated_local_entry_cannot_claim_stored_centrally() {
    let mut archive = zip_with(b"payload that is compressed in the local entry", DEFLATED);

    let central_method = central_header_start(&archive) + 10;
    archive[central_method..central_method + 2].copy_from_slice(&0_u16.to_le_bytes());

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("compression method"),
        "unexpected error: {actual}"
    );
}

// -- Local header must agree with the central directory ------------------

/// Same archive, three readers, three answers: genvm follows the central
/// directory, a streaming reader follows the local header, and CPython rejects
/// the mismatch. Runner ids are content hashes, so that is consensus-visible.
#[test]
fn local_name_cannot_contradict_the_central_directory() {
    let mut archive = zip_with(b"payload contents", genvm_stored_zip::STORED);
    archive[30..37].copy_from_slice(b"evil_id");

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("local name"),
        "unexpected error: {actual}"
    );
}

#[test]
fn encrypted_local_entry_is_rejected() {
    let mut archive = zip_with(b"payload contents", genvm_stored_zip::STORED);
    archive[6..8].copy_from_slice(&1_u16.to_le_bytes());

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("encrypted"),
        "unexpected error: {actual}"
    );
}

#[test]
fn local_crc_cannot_contradict_the_central_directory() {
    let mut archive = zip_with(b"payload contents", genvm_stored_zip::STORED);
    archive[14..18].copy_from_slice(&0xdead_beef_u32.to_le_bytes());

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("local CRC-32"),
        "unexpected error: {actual}"
    );
}

#[test]
fn local_size_cannot_contradict_the_central_directory() {
    let contents = b"payload contents";
    let mut archive = zip_with(contents, genvm_stored_zip::STORED);
    archive[22..26].copy_from_slice(&(contents.len() as u32 + 1).to_le_bytes());

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("local size"),
        "unexpected error: {actual}"
    );
}

// -- Pinned entry fields --------------------------------------------------

fn put_u16(archive: &mut [u8], at: usize, value: u16) {
    archive[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn assert_field_rejected(archive: Vec<u8>, expected: &str) {
    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains(expected),
        "unexpected error: {actual}"
    );
}

fn payload_zip() -> (Vec<u8>, usize) {
    let archive = zip_with(b"payload contents", genvm_stored_zip::STORED);
    let cd = central_header_start(&archive);
    (archive, cd)
}

#[test]
fn version_needed_other_than_2_0_is_rejected() {
    for version in [10, 21, 45] {
        let (mut archive, cd) = payload_zip();
        put_u16(&mut archive, 4, version);
        put_u16(&mut archive, cd + 6, version);
        assert_field_rejected(archive, "needs version");
    }
}

#[test]
fn local_version_cannot_contradict_the_central_directory() {
    let (mut archive, _) = payload_zip();
    put_u16(&mut archive, 4, 45);
    assert_field_rejected(archive, "local version");
}

#[test]
fn local_time_cannot_contradict_the_central_directory() {
    let (mut archive, _) = payload_zip();
    put_u16(&mut archive, 10, 1);
    assert_field_rejected(archive, "local time");
}

#[test]
fn local_date_cannot_contradict_the_central_directory() {
    let (mut archive, _) = payload_zip();
    put_u16(&mut archive, 12, 0x5a62);
    assert_field_rejected(archive, "local date");
}

#[test]
fn any_agreeing_timestamp_is_accepted() {
    let (mut archive, cd) = payload_zip();
    for at in [10, cd + 12] {
        put_u16(&mut archive, at, 0x4a3c);
    }
    for at in [12, cd + 14] {
        put_u16(&mut archive, at, 0x5d62);
    }
    genvm::runners::parse(archive.into(), u32::MAX).unwrap();
}

#[test]
fn entry_on_another_disk_is_rejected() {
    let (mut archive, cd) = payload_zip();
    put_u16(&mut archive, cd + 34, 1);
    assert_field_rejected(archive, "disk");
}

#[test]
fn internal_attributes_are_rejected() {
    let (mut archive, cd) = payload_zip();
    put_u16(&mut archive, cd + 36, 1);
    assert_field_rejected(archive, "internal attributes");
}

#[test]
fn regular_file_attributes_are_accepted() {
    for attributes in [0, 0o600 << 16, 0o644 << 16, 0o100644 << 16, 0o100755 << 16] {
        let (mut archive, cd) = payload_zip();
        put_u32(&mut archive, cd + 38, attributes);
        genvm::runners::parse(archive.into(), u32::MAX).unwrap();
    }
}

#[test]
fn other_external_attributes_are_rejected() {
    let cases = [
        0o120777 << 16, // symlink
        0o040755 << 16, // directory
        0o010644 << 16, // fifo
        0x10,           // DOS directory
        0x20,           // DOS archive
        0o100644 << 16 | 1,
    ];
    for attributes in cases {
        let (mut archive, cd) = payload_zip();
        put_u32(&mut archive, cd + 38, attributes);
        assert_field_rejected(archive, "external attributes");
    }
}

// -- Archives deployed on Bradbury ----------------------------------------

#[test]
fn deployed_zips_are_accepted() {
    let cases: [(&[u8], &[&str]); 2] = [
        (
            include_bytes!("data/zip/bradbury-0x4302.zip"),
            &["contract/__init__.py", "contract/lib.py", "runner.json"],
        ),
        (
            include_bytes!("data/zip/bradbury-0x5711.zip"),
            &[
                "contract/__init__.py",
                "contract/zip_probe.py.xz",
                "runner.json",
            ],
        ),
    ];
    for (archive, names) in cases {
        let actual = genvm::runners::parse(archive.to_vec().into(), u32::MAX).unwrap();
        let actual: Vec<_> = actual.data.keys().map(String::as_str).collect();
        assert_eq!(actual, names);
    }
}
