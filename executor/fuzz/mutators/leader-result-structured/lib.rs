#[path = "../../shared/leader-result-input.rs"]
mod input;

genvm_fuzzing::mutator!(input::Input);
