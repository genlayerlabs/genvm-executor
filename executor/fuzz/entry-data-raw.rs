#[path = "shared/entry-data.rs"]
mod shared;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        shared::assert_validate_main_matches_reference(data);
    });
}
