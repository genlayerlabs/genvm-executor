#[path = "../../shared/storage-input.rs"]
mod input;

genvm_fuzzing::mutator!(input::FuzzInput);
