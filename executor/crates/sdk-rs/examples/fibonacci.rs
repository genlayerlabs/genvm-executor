//! Example contract that only answers as a sandbox child: it computes a
//! Fibonacci number and hands it back as bytes. A contract in another language
//! can register this module as a runner at runtime and call into it.

use genlayer_sdk::abi::entry::MessageData;
use genlayer_sdk::abi::entry::contract_def::Contract;
use genlayer_sdk::calldata::Value;

const N: u32 = 30;

fn fibonacci(n: u32) -> u64 {
    let (mut prev, mut cur) = (0u64, 1u64);
    for _ in 0..n {
        (prev, cur) = (cur, prev + cur);
    }
    prev
}

#[derive(Default)]
pub struct Fibonacci;

impl Contract for Fibonacci {
    fn handle_main(&mut self, _message: MessageData, _data: bytes::Bytes) -> Result<Value, String> {
        Err("this runner is only callable as a sandbox".to_owned())
    }

    fn handle_sandbox(
        &mut self,
        _message: MessageData,
        _data: bytes::Bytes,
    ) -> Result<Vec<u8>, String> {
        Ok(fibonacci(N).to_string().into_bytes())
    }

    fn handle_consensus_stage(
        &mut self,
        _message: MessageData,
        _data: bytes::Bytes,
        _stage_data: genlayer_sdk::abi::entry::contract_def::ConsensusStageData,
    ) -> Result<Value, String> {
        Err("this runner is only callable as a sandbox".to_owned())
    }
}

genlayer_sdk::contract_main!(Fibonacci);
