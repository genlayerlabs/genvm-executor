#[path = "shared/leader-result.rs"]
mod shared;

#[path = "shared/leader-result-input.rs"]
mod input;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let Some(input) = genvm_fuzzing::decode::<input::Input>(data) else {
            return;
        };
        shared::assert_parse_properties(&input.to_bytes());
    });
}
