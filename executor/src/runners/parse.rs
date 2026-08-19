use crate::rt;
use crate::rt::errors::Error;
use genlayer_sdk::abi;
use genvm_common::*;

/// What a raw wasm module may say about itself, in custom sections. Both are
/// optional and both fall back to a default: the current major, and a runner
/// that just starts this module.
#[derive(Default)]
struct WasmSelfDescription {
    version: Option<String>,
    runner_json: Option<bytes::Bytes>,
}

const DEFAULT_RUNNER_JSON: &[u8] = b"{ \"StartWasm\": \"file\" }";

fn describe_wasm(code: &[u8]) -> rt::errors::Result<WasmSelfDescription> {
    let mut res = WasmSelfDescription::default();

    for payload in wasmparser::Parser::new(0).parse_all(code) {
        let wasmparser::Payload::CustomSection(section) = payload? else {
            continue;
        };
        match section.name() {
            // an unreadable section is treated as absent rather than as a
            // parse failure, so it cannot take the other section down with it
            "genvm.version" => match std::str::from_utf8(section.data()) {
                Ok(version) => res.version = Some(version.to_owned()),
                Err(e) => log_warn!(error:err = e; "invalid utf-8 in the version section"),
            },
            "genvm.runner.json" => {
                res.runner_json = Some(bytes::Bytes::copy_from_slice(section.data()));
            }
            _ => {}
        }
    }

    Ok(res)
}

pub fn parse(code: bytes::Bytes) -> rt::errors::Result<super::Archive> {
    if let Ok(mut as_zip) = zip::ZipArchive::new(std::io::Cursor::new(code.clone())) {
        return super::Archive::from_zip(&mut as_zip, code);
    }

    if wasmparser::Parser::is_core_wasm(code.as_ref()) {
        let described = match describe_wasm(code.as_ref()) {
            Ok(v) => v,
            Err(e) => {
                log_warn!(default = host_fns::CURRENT_MAJOR_STR, error = e; "could not read wasm custom sections");
                WasmSelfDescription::default()
            }
        };
        let version = described
            .version
            .unwrap_or_else(|| host_fns::CURRENT_MAJOR_STR.to_owned());
        let runner_json = described
            .runner_json
            .unwrap_or_else(|| bytes::Bytes::from_static(DEFAULT_RUNNER_JSON));
        return Ok(super::Archive::from_file_and_runner(
            code,
            bytes::Bytes::copy_from_slice(version.as_bytes()),
            runner_json,
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
            abi::consts::VmError::invalid_contract().runner().absent(),
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
