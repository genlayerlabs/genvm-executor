use crate::rt;
use crate::rt::errors::Error;
use genlayer_sdk::abi;
use genvm_common::*;

fn detect_version_from_wasm(code: &[u8]) -> rt::errors::Result<String> {
    let parser = wasmparser::Parser::new(0);

    for payload in parser.parse_all(code) {
        match payload? {
            wasmparser::Payload::CustomSection(section) if section.name() == "genvm.version" => {
                let version = section.data().to_vec();
                if let Ok(version_str) = std::str::from_utf8(&version) {
                    return Ok(version_str.to_string());
                } else {
                    return Err(Error::internal("Invalid UTF-8 in version section"));
                }
            }
            _ => {}
        }
    }

    Err(Error::internal("version section not found"))
}

pub fn parse(code: bytes::Bytes) -> rt::errors::Result<super::Archive> {
    if let Ok(mut as_zip) = zip::ZipArchive::new(std::io::Cursor::new(code.clone())) {
        return super::Archive::from_zip(&mut as_zip, code);
    }

    if wasmparser::Parser::is_core_wasm(code.as_ref()) {
        let version = match detect_version_from_wasm(code.as_ref()) {
            Ok(v) => v,
            Err(e) => {
                log_warn!(default = host_fns::CURRENT_MAJOR_STR, error = e; "could not detect version from wasm");
                host_fns::CURRENT_MAJOR_STR.to_string()
            }
        };
        return Ok(super::Archive::from_file_and_runner(
            code,
            bytes::Bytes::copy_from_slice(version.as_bytes()),
            bytes::Bytes::from_static(b"{ \"StartWasm\": \"file\" }"),
        ));
    }

    code_to_archive_from_text(code)
}

fn code_to_archive_from_text(code: bytes::Bytes) -> rt::errors::Result<super::Archive> {
    let code_str = std::str::from_utf8(code.as_ref()).map_err(|e| {
        Error::vm(abi::consts::VmError::invalid_contract().not_utf8_text()).with_source(e)
    })?;

    let code_start = (|| {
        for c in ["//", "#", "--"] {
            if code_str.starts_with(c) {
                return Ok(c);
            }
        }
        Err(rt::errors::Error::vm(
            abi::consts::VmError::invalid_contract().absent_runner_comment(),
        ))
    })()?;

    let mut version_string = String::new();
    let mut code_comment = String::new();
    let mut first = true;
    for l in code_str.lines() {
        if !l.starts_with(code_start) {
            break;
        }

        let l = &l[code_start.len()..];

        if first {
            first = false;
            if l.trim().starts_with("v") {
                version_string.push_str(l);
            } else {
                version_string.push_str(host_fns::CURRENT_MAJOR_STR);

                code_comment.push_str(l)
            }
        } else {
            code_comment.push_str(l)
        }
    }

    Ok(super::Archive::from_file_and_runner(
        code,
        bytes::Bytes::copy_from_slice(version_string.as_bytes()),
        bytes::Bytes::copy_from_slice(code_comment.as_bytes()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    const BLOCK_SIZE: usize = 512;

    fn ustar(name: &[u8], prefix: &[u8], contents: &[u8]) -> bytes::Bytes {
        ustar_with_type(name, prefix, contents, b'0')
    }

    fn ustar_with_type(name: &[u8], prefix: &[u8], contents: &[u8], type_flag: u8) -> bytes::Bytes {
        assert!(name.len() <= 100);
        assert!(prefix.len() <= 155);

        let mut header = [0u8; BLOCK_SIZE];
        header[..name.len()].copy_from_slice(name);
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        let size = format!("{:011o}\0", contents.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = type_flag;
        header[257..265].copy_from_slice(b"ustar\x0000".as_slice());
        header[345..345 + prefix.len()].copy_from_slice(prefix);
        let checksum: u32 = header.iter().map(|byte| *byte as u32).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());

        let padded_contents = contents.len().div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
        let mut archive = Vec::with_capacity(BLOCK_SIZE + padded_contents + 2 * BLOCK_SIZE);
        archive.extend_from_slice(&header);
        archive.extend_from_slice(contents);
        archive.resize(BLOCK_SIZE + padded_contents, 0);
        archive.resize(BLOCK_SIZE + padded_contents + 2 * BLOCK_SIZE, 0);
        archive.into()
    }

    #[test]
    fn runtime_parser_accepts_ustar() {
        let archive = parse(ustar(b"runner.json", b"", br#"{"StartWasm":"file"}"#)).unwrap();
        assert_eq!(
            archive.data.get("runner.json").unwrap().as_ref(),
            br#"{"StartWasm":"file"}"#
        );
    }

    #[test]
    fn ustar_prefix_is_separated_from_name() {
        let archive =
            super::super::Archive::from_ustar(ustar(b"file", b"nested", b"value")).unwrap();
        assert_eq!(
            archive.data.keys().map(String::as_str).collect::<Vec<_>>(),
            ["nested/file"],
            "USTAR prefix and name fields must be joined with a slash"
        );
    }

    #[test]
    fn full_width_ustar_name_is_not_truncated() {
        let name = [b'x'; 100];
        let archive = super::super::Archive::from_ustar(ustar(&name, b"", b"value")).unwrap();
        assert_eq!(
            archive.data.keys().next().unwrap().len(),
            name.len(),
            "a full-width USTAR name field must retain its final byte"
        );
    }

    #[test]
    fn ustar_name_stops_at_nul_terminator() {
        let archive =
            super::super::Archive::from_ustar(ustar(b"file\0ignored", b"", b"value")).unwrap();

        assert_eq!(
            archive.data.keys().map(String::as_str).collect::<Vec<_>>(),
            ["file"],
            "USTAR names must stop at the first NUL in the fixed-width name field"
        );
    }

    #[test]
    fn ustar_directory_type_is_not_exposed_as_a_file() {
        let archive =
            super::super::Archive::from_ustar(ustar_with_type(b"nested", b"", b"", b'5')).unwrap();
        assert!(
            archive.data.is_empty(),
            "USTAR type flag `5` denotes a directory; got entries {:?}",
            archive.data.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn ustar_rejects_a_header_with_a_bad_checksum() {
        let mut archive = ustar(b"runner.json", b"", br#"{"StartWasm":"file"}"#).to_vec();
        archive[0] = b'R';

        assert!(
            super::super::Archive::from_ustar(archive.into()).is_err(),
            "a USTAR header modified without updating its checksum must be rejected"
        );
    }

    #[test]
    fn ustar_rejects_missing_end_markers() {
        let mut archive = ustar(b"runner.json", b"", br#"{"StartWasm":"file"}"#).to_vec();
        archive.truncate(archive.len() - 2 * BLOCK_SIZE);

        assert!(
            super::super::Archive::from_ustar(archive.into()).is_err(),
            "a truncated USTAR archive without its two zero end markers must be rejected"
        );
    }

    #[test]
    fn zip_rejects_stored_contents_with_a_bad_crc() {
        let contents = b"payload whose CRC must be checked";
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            writer
                .start_file(
                    "payload",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(contents).unwrap();
            writer.finish().unwrap();
        }

        let mut archive = cursor.into_inner();
        let contents_offset = archive
            .windows(contents.len())
            .position(|window| window == contents)
            .unwrap();
        archive[contents_offset] ^= 1;

        assert!(
            parse(archive.into()).is_err(),
            "stored ZIP contents modified without updating their CRC must be rejected"
        );
    }
}
