/// Contract code, in the shapes `runners::parse` dispatches on beyond raw bytes:
/// random bytes almost never form a zip or a wasm module, so the deeper parsers
/// would otherwise go unvisited. The raw shape is its own target.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize, mutatis::Mutate)]
pub enum Input {
    #[default]
    Empty,
    Text(String),
    Wasm {
        version: Option<Vec<u8>>,
        runner_json: Option<Vec<u8>>,
    },
    Zip(Vec<(String, Vec<u8>)>),
}
