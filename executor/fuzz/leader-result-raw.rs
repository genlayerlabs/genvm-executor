#[path = "shared/leader-result.rs"]
mod shared;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        shared::assert_parse_properties(data);
    });
}
