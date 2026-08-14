#[path = "shared/runners-parse.rs"]
mod shared;

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("current thread runtime");

    afl::fuzz!(|data: &[u8]| {
        shared::assert_parse_properties(&runtime, data.to_vec());
    });
}
