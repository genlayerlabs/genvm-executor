#[path = "shared/entry-data.rs"]
mod shared;

use genlayer_calldata::fuzzing::Corpus;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let Some(Corpus(value)) = genvm_fuzzing::decode(data) else {
            return;
        };
        shared::assert_validate_main_matches_reference(&genvm::calldata::encode(&value));
    });
}
