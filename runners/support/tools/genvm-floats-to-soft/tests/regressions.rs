use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, MemoryType, Module, TypeSection, ValType,
};

static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(0);

fn f32_add_module(import_memory: bool) -> Vec<u8> {
    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.function([ValType::F32, ValType::F32], [ValType::F32]);
    module.section(&types);

    if import_memory {
        let mut imports = ImportSection::new();
        imports.import(
            "env",
            "memory",
            EntityType::Memory(MemoryType {
                minimum: 1,
                maximum: None,
                memory64: false,
                shared: false,
                page_size_log2: None,
            }),
        );
        module.section(&imports);
    }

    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);

    let mut exports = ExportSection::new();
    exports.export("add", ExportKind::Func, 0);
    module.section(&exports);

    let mut function = Function::new([]);
    function.instruction(&Instruction::LocalGet(0));
    function.instruction(&Instruction::LocalGet(1));
    function.instruction(&Instruction::F32Add);
    function.instruction(&Instruction::End);
    let mut code = CodeSection::new();
    code.function(&function);
    module.section(&code);

    module.finish()
}

fn rewrite(input: &[u8], case: &str) -> Vec<u8> {
    let id = NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed);
    let prefix = format!("genvm-floats-to-soft-{case}-{}-{id}", std::process::id());
    let input_path = std::env::temp_dir().join(format!("{prefix}.wasm"));
    let output_path = std::env::temp_dir().join(format!("{prefix}.rewritten.wasm"));
    std::fs::write(&input_path, input).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_genvm-floats-to-soft"))
        .arg(&input_path)
        .arg(&output_path)
        .output()
        .unwrap();
    let rewritten = std::fs::read(&output_path).ok();

    let _ = std::fs::remove_file(input_path);
    let _ = std::fs::remove_file(output_path);

    assert!(
        output.status.success(),
        "rewriter rejected valid Wasm: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    rewritten.expect("successful rewriter did not create its output")
}

fn rewritten_indices(bytes: &[u8]) -> (u32, u32, u32) {
    let mut function_import_count = 0;
    let mut f32_add_index = None;
    let mut add_export_index = None;
    let mut rewritten_call_index = None;

    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            wasmparser::Payload::ImportSection(section) => {
                for import in section {
                    let import = import.unwrap();
                    if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                        if import.module == "softfloat" && import.name == "f32_add" {
                            f32_add_index = Some(function_import_count);
                        }
                        function_import_count += 1;
                    }
                }
            }
            wasmparser::Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.name == "add" {
                        add_export_index = Some(export.index);
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if let wasmparser::Operator::Call { function_index } = reader.read().unwrap() {
                        rewritten_call_index = Some(function_index);
                    }
                }
            }
            _ => {}
        }
    }

    (
        f32_add_index.unwrap(),
        add_export_index.unwrap(),
        rewritten_call_index.unwrap(),
    )
}

fn assert_f32_add_rewritten(input: Vec<u8>, case: &str) {
    let rewritten = rewrite(&input, case);
    let (f32_add_index, add_export_index, rewritten_call_index) = rewritten_indices(&rewritten);

    assert_eq!(
        rewritten_call_index, f32_add_index,
        "f32.add must call softfloat.f32_add"
    );
    assert!(
        add_export_index > f32_add_index,
        "the export must still point to the local function"
    );
}

#[test]
fn regression_rewrites_module_without_an_import_section() {
    assert_f32_add_rewritten(f32_add_module(false), "no-import-section");
}

#[test]
fn regression_non_function_imports_do_not_shift_function_indices() {
    assert_f32_add_rewritten(f32_add_module(true), "imported-memory");
}
