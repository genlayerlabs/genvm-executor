#[derive(Debug, serde::Serialize, serde::Deserialize, mutatis::Mutate)]
pub enum Op {
    Insert { key: u32, value: u32 },
    Remove { key: u32 },
    Get { key: u32 },
}
