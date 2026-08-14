#[path = "shared/runners-parse.rs"]
mod shared;

#[path = "shared/runners-parse-input.rs"]
mod input;

use input::Input;

fn leb128(mut value: usize, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn custom_section(name: &str, data: &[u8], out: &mut Vec<u8>) {
    let mut payload = Vec::new();
    leb128(name.len(), &mut payload);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);

    out.push(0);
    leb128(payload.len(), out);
    out.extend_from_slice(&payload);
}

fn wasm_bytes(version: Option<&[u8]>, runner_json: Option<&[u8]>) -> Vec<u8> {
    let mut out = b"\0asm\x01\0\0\0".to_vec();
    if let Some(version) = version {
        custom_section("genvm.version", version, &mut out);
    }
    if let Some(runner_json) = runner_json {
        custom_section("genvm.runner.json", runner_json, &mut out);
    }
    out
}

fn zip_bytes(entries: &[(String, Vec<u8>)]) -> Option<Vec<u8>> {
    use std::io::Write as _;

    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer.start_file(name.as_str(), options).ok()?;
        writer.write_all(data).ok()?;
    }
    Some(writer.finish().ok()?.into_inner())
}

fn code_from(input: &Input) -> Option<Vec<u8>> {
    match input {
        Input::Empty => Some(Vec::new()),
        Input::Text(text) => Some(text.as_bytes().to_vec()),
        Input::Wasm {
            version,
            runner_json,
        } => Some(wasm_bytes(version.as_deref(), runner_json.as_deref())),
        Input::Zip(entries) => zip_bytes(entries),
    }
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("current thread runtime");

    afl::fuzz!(|data: &[u8]| {
        let Some(input) = genvm_fuzzing::decode::<Input>(data) else {
            return;
        };
        let Some(code) = code_from(&input) else {
            return;
        };
        shared::assert_parse_properties(&runtime, code);
    });
}
