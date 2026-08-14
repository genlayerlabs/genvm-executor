#[path = "../../shared/tree-map-op.rs"]
mod shared;

genvm_fuzzing::mutator!(Vec<shared::Op>);
