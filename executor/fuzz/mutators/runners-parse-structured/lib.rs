#[path = "../../shared/runners-parse-input.rs"]
mod input;

genvm_fuzzing::mutator!(input::Input);
