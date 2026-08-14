// Slots are named by one byte, not by the 32 the storage layer uses: two random
// 32 byte slots never collide, so the reference and the implementation were only
// ever compared on disjoint slots. A byte makes reuse -- and the paging that goes
// with it -- the common case.
pub fn slot_bytes(slot: u8) -> [u8; 32] {
    let mut raw = [0u8; 32];
    raw[0] = slot;
    raw
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, mutatis::Mutate)]
pub struct FuzzInput {
    pub initial_data: Vec<Entry>,
    pub operations: Vec<Op>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, mutatis::Mutate)]
pub struct Entry {
    pub slot: u8,
    pub index: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, mutatis::Mutate)]
pub enum Op {
    #[default]
    Nop,
    Write {
        slot: u8,
        index: u32,
        data: Vec<u8>,
    },
    Read {
        slot: u8,
        index: u32,
        len: u8,
    },
}
