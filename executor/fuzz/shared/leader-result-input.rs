use genlayer_calldata::fuzzing::Corpus;

// The frame byte stays orthogonal to the payload: a code that disagrees with
// what follows it is the boundary this target exists to reach.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize, mutatis::Mutate)]
pub struct Input {
    pub code: u8,
    pub payload: Payload,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, mutatis::Mutate)]
pub enum Payload {
    #[default]
    Empty,
    Calldata(Corpus),
    Text(String),
    Bytes(Vec<u8>),
}

impl Input {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = vec![self.code];
        match &self.payload {
            Payload::Empty => {}
            Payload::Calldata(Corpus(value)) => {
                data.extend_from_slice(&genlayer_calldata::encode(value))
            }
            Payload::Text(text) => data.extend_from_slice(text.as_bytes()),
            Payload::Bytes(bytes) => data.extend_from_slice(bytes),
        }
        data
    }
}
