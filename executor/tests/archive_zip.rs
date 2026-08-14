use std::io::Write as _;

fn zip_with(contents: &[u8], method: zip::CompressionMethod) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        writer
            .start_file(
                "payload",
                zip::write::SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        writer.write_all(contents).unwrap();
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

fn stored_zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        for (name, contents) in entries {
            writer
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

/// The zip writer refuses duplicate names, so two distinct entries are renamed
/// to a common one afterwards. Both names are the same length, which keeps every
/// offset in the archive valid.
fn zip_with_duplicate_names() -> Vec<u8> {
    let mut archive = stored_zip_with_entries(&[("payload1", b"first"), ("payload2", b"second")]);
    while let Some(at) = archive.windows(8).position(|window| window == b"payload2") {
        archive[at..at + 8].copy_from_slice(b"payload1");
    }
    archive
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
    match genvm::runners::parse(archive.into()) {
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

/// Which of two same-named entries a runner ends up with decides what every
/// validator executes, so it is pinned here rather than left to whatever the zip
/// crate happens to do after an upgrade.
#[test]
fn duplicate_zip_entry_names_resolve_to_the_last_entry() {
    let actual = genvm::runners::parse(zip_with_duplicate_names().into()).unwrap();
    assert!(
        actual.data.len() == 1 && actual.data.get("payload1").is_some_and(|v| v == "second"),
        "unexpected entries: {:?}",
        actual.data
    );
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
    let mut archive = zip_with(b"crc payload", zip::CompressionMethod::Stored);
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
fn zip_directory_entry_is_skipped() {
    let archive = stored_zip_with_entries(&[("dir/", b""), ("dir/payload", b"contents")]);
    let actual = genvm::runners::parse(archive.into()).unwrap();
    assert!(
        actual.data.len() == 1 && actual.data.contains_key("dir/payload"),
        "unexpected entries: {:?}",
        actual.data.keys()
    );
}

#[test]
fn zip_directory_entry_with_contents_is_rejected() {
    let archive = stored_zip_with_entries(&[("dir/", b"contents")]);
    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("directory"),
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

#[test]
fn invalid_directory_names_are_rejected() {
    for name in ["dir//", "./dir/"] {
        let actual = error_of(stored_zip_with_entries(&[(name, b"")]));
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
    let mut archive = zip_with(contents, zip::CompressionMethod::Stored);
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
    let mut archive = zip_with(
        b"payload that is compressed in the local entry",
        zip::CompressionMethod::Deflated,
    );

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
    let mut archive = zip_with(b"payload contents", zip::CompressionMethod::Stored);
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
    let mut archive = zip_with(b"payload contents", zip::CompressionMethod::Stored);
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
    let mut archive = zip_with(b"payload contents", zip::CompressionMethod::Stored);
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
    let mut archive = zip_with(contents, zip::CompressionMethod::Stored);
    archive[22..26].copy_from_slice(&(contents.len() as u32 + 1).to_le_bytes());

    let actual = error_of(archive);
    assert_malformed_runner(&actual);
    assert!(
        actual.to_string().contains("local size"),
        "unexpected error: {actual}"
    );
}
