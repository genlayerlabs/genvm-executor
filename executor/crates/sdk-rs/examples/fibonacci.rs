//! Example contract that only answers as a sandbox child: it reads `n` from its
//! entry payload and hands back the n-th Fibonacci number. A contract in another
//! language can register this module as a runner at runtime and call into it.
//!
//! Both directions of the boundary are `calldata`, so neither side has to agree
//! on a bespoke byte layout.

use genlayer_sdk::abi::entry::MessageData;
use genlayer_sdk::abi::entry::contract_def::Contract;
use genlayer_sdk::calldata::{self, Value};

fn fibonacci(n: u64) -> num_bigint::BigInt {
    let (mut prev, mut cur) = (num_bigint::BigInt::from(0), num_bigint::BigInt::from(1));
    for _ in 0..n {
        (prev, cur) = (cur.clone(), prev + cur);
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
        data: bytes::Bytes,
    ) -> Result<Vec<u8>, String> {
        let Ok(Value::Number(n)) = calldata::decode(&data) else {
            return Err("entry payload is not a calldata number".to_owned());
        };
        let n = u64::try_from(n).map_err(|_| "n is out of range".to_owned())?;

        Ok(calldata::encode(&Value::Number(fibonacci(n))))
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
