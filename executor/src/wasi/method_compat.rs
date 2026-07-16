//! Method-name key compatibility between the v0.3-derived executor (new key "")
//! and the prebuilt v0.2.x python runner, which we cannot modify and which
//! reads/writes the contract method name under the legacy key "method"
//! (`_genlayer_runner.py:106` `ctx.cd.get('method', '')`,
//! `gl/genvm_contracts.py:46` `ret['method'] = method`).
//!
//! The executor keeps its in-memory message data on the new "" key; only the
//! bytes handed to the runner (and thus the determinism seed derived from them)
//! are rewritten to the legacy "method" key.

use crate::calldata;

pub const NEW_METHOD_KEY: &str = "";
pub const LEGACY_METHOD_KEY: &str = "method";

/// Move a contract-call calldata map's method-name entry from `from` to `to`.
/// No-op if `from` is absent or `from == to`. Idempotent: re-running cannot
/// double-move because `from` no longer exists after the first move.
fn move_method_key(map: &mut calldata::Map, from: &str, to: &str) {
    if from == to {
        return;
    }
    if let Some(v) = map.remove(from) {
        map.insert(to.to_owned(), v);
    }
}

/// NEW ("") -> LEGACY ("method"). Applied to the calldata map just before the
/// message is encoded for the runner.
pub fn method_new_to_legacy(value: &mut calldata::Value) {
    if let calldata::Value::Map(map) = value {
        move_method_key(map, NEW_METHOD_KEY, LEGACY_METHOD_KEY);
    }
}

/// LEGACY ("method") -> NEW (""). Inverse; provided for symmetry and for any
/// future path that must surface runner-built calldata on the new interface.
pub fn method_legacy_to_new(value: &mut calldata::Value) {
    if let calldata::Value::Map(map) = value {
        move_method_key(map, LEGACY_METHOD_KEY, NEW_METHOD_KEY);
    }
}

/// Decode `entry_data`, rewrite new->legacy, re-encode. Returns the input
/// unchanged if it does not decode to a calldata map (callers still gate on
/// `EntryKind::Main`; this is defense in depth for opaque entry data).
pub fn entry_data_new_to_legacy(entry_data: bytes::Bytes) -> bytes::Bytes {
    let mut value = match calldata::decode(&entry_data) {
        Ok(v @ calldata::Value::Map(_)) => v,
        _ => return entry_data,
    };
    method_new_to_legacy(&mut value);
    bytes::Bytes::from(calldata::encode(&value))
}
