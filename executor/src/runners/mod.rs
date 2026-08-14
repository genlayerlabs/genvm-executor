pub mod actions;
pub mod cache;

mod archive;
mod parse;
pub(crate) mod ustar;
use genvm_common::*;
use itertools::Itertools;

pub use archive::Archive;

pub use parse::parse;

pub use actions::*;

use std::{str::FromStr as _, sync::Arc};

use crate::rt;
use crate::rt::errors::{Error, ResultExt as _};

pub(crate) fn malformed_runner_error(message: impl Into<String>) -> Error {
    Error::vm(
        genlayer_sdk::abi::consts::VmError::invalid_contract()
            .runner()
            .malformed(),
    )
    .context(message)
}

pub fn append_runner_subpath(id: &str, hash: &str, path: &mut std::path::PathBuf) {
    path.push(id);
    path.push(&hash[..2]);
    path.push(&hash[2..]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, genlayer_calldata::Encode)]
pub enum ChainState {
    Accepted,
    Finalized,
    Deploy,
}

impl ChainState {
    pub fn for_vm(is_init: bool, state: crate::public_abi::StorageType) -> Self {
        if is_init {
            ChainState::Deploy
        } else {
            match state {
                crate::public_abi::StorageType::LatestFinal => ChainState::Finalized,
                _ => ChainState::Accepted,
            }
        }
    }

    pub fn host_storage_type(self) -> Option<crate::public_abi::StorageType> {
        match self {
            ChainState::Accepted => Some(crate::public_abi::StorageType::LatestNonFinal),
            ChainState::Finalized => Some(crate::public_abi::StorageType::LatestFinal),
            ChainState::Deploy => None,
        }
    }

    fn canonical_char(self) -> char {
        match self {
            ChainState::Accepted => 'a',
            ChainState::Finalized => 'f',
            ChainState::Deploy => 'd',
        }
    }
}

/// A parsed runner id. Runner ids come in the following forms:
///
/// - `name:hash` -- a packaged runner identified by its human readable name and
///   content hash (in debug mode `hash` may also be `test`/`latest`).
/// - `contract` -- the runner of the contract that is currently being executed.
/// - `chain:<address>:<a|f>:<slot>` -- read the runner code blob from a storage
///   slot of an arbitrary contract. `address` is a `0x`-prefixed 20 byte hex
///   address, `a`/`f` selects accepted (latest non final) / finalized state and
///   `slot` is a 32 byte slot id encoded with GVM32 (Crockford Base32).
///   Both `<a|f>` and `<slot>` are optional: `<a|f>` defaults to `a` and
///   `<slot>` defaults to reading the target contract's root slot during
///   resolution (i.e. `chain:<address>` and `chain:<address>:a` are valid).
/// - `custom:<hash>` -- a runner registered at runtime, looked up by its hash.
#[derive(Debug)]
pub enum IdUnresolved {
    Builtin {
        name: String,
        hash: String,
    },
    Contract,
    Chain {
        address: calldata::Address,
        on: Option<ChainState>,
        slot: Option<crate::SlotID>,
    },
    Custom {
        hash: String,
    },
}

#[derive(Debug, Clone)]
pub enum Id {
    Builtin {
        name: symbol_table::GlobalSymbol,
        hash: Bytes32Hash,
    },
    Chain {
        address: calldata::Address,
        on: ChainState,
        slot: crate::SlotID,
    },
    Custom {
        hash: Bytes32Hash,
    },
}

impl serde::Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.canonical().as_str())
    }
}

impl<W: calldata::Writer> calldata::codec::Encode<W> for Id {
    type Error = <W as calldata::Writer>::Error;

    fn encode(&self, encoder: &mut genlayer_calldata::Encoder<W>) -> Result<(), Self::Error> {
        let canonical = self.canonical();
        encoder.push_str(canonical.as_str())
    }
}

impl Id {
    /// The canonical textual runner id, used as the archive cache key. Two ids
    /// that resolve to the same `(name, hash)` / `(address, state, slot)` / hash
    /// produce the same symbol (e.g. `contract` and an explicit self `chain:`).
    pub fn canonical(&self) -> symbol_table::GlobalSymbol {
        match self {
            Id::Builtin { name, hash } => {
                let mut id = name.as_str().to_owned();
                id.push(':');
                id.push_str(&hash.to_gvm32());
                symbol_table::GlobalSymbol::from(id)
            }
            Id::Chain { address, on, slot } => symbol_table::GlobalSymbol::from(format!(
                "chain:0x{}:{}:{}",
                address.checksum_hex_string(),
                on.canonical_char(),
                genlayer_sdk::gvm32::encode(&slot.raw())
            )),
            Id::Custom { hash } => {
                let mut id = String::from("custom:");
                id.push_str(&hash.to_gvm32());
                symbol_table::GlobalSymbol::from(id)
            }
        }
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.canonical().as_str())
    }
}

fn is_hash_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '='
}

fn parse_safe_address(s: &str) -> Option<[u8; 20]> {
    let s = s.strip_prefix("0x").unwrap_or(s);

    if s.len() != 40 {
        log_warn!("address must be exactly 40 hex characters, got {}", s.len());
        return None;
    }

    let mut address = [0u8; 20];
    if hex::decode_to_slice(s, &mut address).is_err() {
        log_warn!("address contains invalid hex characters");
        return None;
    }

    let is_all_lower = s.chars().all(|c| !c.is_ascii_uppercase());
    let is_all_upper = s.chars().all(|c| !c.is_ascii_lowercase());

    if is_all_lower || is_all_upper {
        return Some(address);
    }

    let addr = calldata::Address::from(address);
    let checksum = addr.checksum_hex_string();
    if s == checksum {
        return Some(address);
    }

    log_warn!("address must be all lowercase, all uppercase, or valid checksum");
    None
}

pub fn parse_runner_id(id: &str) -> Option<IdUnresolved> {
    if id == "contract" {
        return Some(IdUnresolved::Contract);
    }

    if let Some(rest) = id.strip_prefix("chain:") {
        let mut it = rest.split(':');
        let address = it.next()?;
        let on_str = it.next();
        let slot_str = it.next();
        if it.next().is_some() {
            return None;
        }

        if !address.starts_with("0x") {
            log_warn!("chain address must be 0x-prefixed");
            return None;
        }

        let address = calldata::Address::from(parse_safe_address(address)?);
        let on = match on_str {
            Some("a") | None => Some(ChainState::Accepted),
            Some("f") => Some(ChainState::Finalized),
            _ => return None,
        };
        let slot = match slot_str {
            Some(s) => {
                let bytes: [u8; 32] = genlayer_sdk::gvm32::decode(s).ok()?.try_into().ok()?;
                Some(crate::SlotID::from_bytes(bytes))
            }
            None => None,
        };

        return Some(IdUnresolved::Chain { address, on, slot });
    }

    if let Some(rest) = id.strip_prefix("custom:") {
        if rest.is_empty() || !rest.chars().all(is_hash_char) {
            return None;
        }
        return Some(IdUnresolved::Custom {
            hash: rest.to_owned(),
        });
    }

    let (name, hash) = id.split_once(':')?;
    if name.is_empty() || hash.is_empty() {
        return None;
    }
    // The runner-id scheme reserves `contract`, `chain` and `custom` as prefixes. `chain:`/`custom:`
    // are already handled above; a `contract:<hash>` id must not fall through to the
    // generic builtin arm (the bare `contract` literal is handled at the top).
    if matches!(name, "contract" | "chain" | "custom") {
        log_warn!("`{name}` is a reserved runner name and cannot be used as a builtin");
        return None;
    }
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' {
            log_warn!("character `{c}` is not allowed in runner id");
            return None;
        }
    }
    if !hash.chars().all(is_hash_char) {
        return None;
    }
    Some(IdUnresolved::Builtin {
        name: name.to_owned(),
        hash: hash.to_owned(),
    })
}

#[cfg(test)]
mod parse_runner_id_tests {
    use super::*;

    #[test]
    fn bare_contract_literal_is_special_cased() {
        assert!(matches!(
            parse_runner_id("contract"),
            Some(IdUnresolved::Contract)
        ));
    }

    #[test]
    fn contract_is_a_reserved_builtin_name() {
        // The runner-id scheme reserves `contract`; a `contract:<hash>` id must not fall through to
        // the generic `name:hash` builtin arm.
        assert!(parse_runner_id("contract:abc123").is_none());
    }

    #[test]
    fn ordinary_builtin_still_parses() {
        assert!(matches!(
            parse_runner_id("py:abc123"),
            Some(IdUnresolved::Builtin { name, hash })
                if name == "py" && hash == "abc123"
        ));
    }
}

/// The dev-mode magic builtin/custom runner hash. Its GVM32 form is the literal
/// string `test` padded with `0`s to 52 chars (`test0000...`), which is what shows
/// up on disk and in `all.json`/`latest.json`. The raw bytes below are exactly
/// `gvm32::decode("test0000...")`. Kept in sync with `hashToIDHash` in
/// `runners/support/default.nix`.
pub const TEST_RUNNER_HASH: Bytes32Hash = {
    let mut bytes = [0u8; 32];
    bytes[0] = 0xd3;
    bytes[1] = 0xb3;
    bytes[2] = 0xa0;
    Bytes32Hash::from_bytes(bytes)
};

pub fn custom_runner_hash(code: &[u8]) -> Bytes32Hash {
    use sha3::Digest as _;
    let mut digest = sha3::Sha3_256::new();
    digest.update(code);
    let arr: [u8; 32] = digest.finalize().into();
    Bytes32Hash::from_bytes(arr)
}

pub struct ArchiveCache {
    pub(super) id: symbol_table::GlobalSymbol,
    pub(super) files: Archive,
    pub(super) actions: tokio::sync::OnceCell<Arc<InitAction>>,
}

impl ArchiveCache {
    pub fn runner_id(&self) -> symbol_table::GlobalSymbol {
        self.id
    }

    pub fn new(id: symbol_table::GlobalSymbol, files: Archive) -> Self {
        Self {
            id,
            files,
            actions: tokio::sync::OnceCell::new(),
        }
    }

    pub fn get_version(&self) -> rt::errors::Result<genvm_common::version::Version> {
        let contents = match self.get_file("version") {
            Ok(contents) => contents,
            Err(e) => {
                log_warn!(error:err = e, runner = self.id; "failed to read version file for runner, using default");
                bytes::Bytes::copy_from_slice(host_fns::CURRENT_MAJOR_STR.as_bytes())
            }
        };

        let contents = std::str::from_utf8(contents.as_ref()).map_err(|e| {
            malformed_runner_error(format!("version is not valid UTF-8 for runner {}", self.id))
                .with_source(e)
        })?;

        let version = version::Version::from_str(contents).map_err(|e| {
            Error::wrap(
                genlayer_sdk::abi::consts::VmError::invalid_contract()
                    .runner()
                    .malformed(),
                e,
            )
            .context(format!(
                "parsing version '{}' for runner {}",
                contents.trim(),
                self.id
            ))
        })?;

        log_trace!(from = contents, to = version; "version parsed");

        Ok(version)
    }

    pub async fn get_actions(&self) -> rt::errors::Result<Arc<InitAction>> {
        self.actions
            .get_or_try_init(|| async {
                let contents = self.get_file("runner.json")?;

                let contents_str = std::str::from_utf8(contents.as_ref()).map_err(|e| {
                    malformed_runner_error(format!(
                        "runner.json is not valid UTF-8 for runner {}",
                        self.id
                    ))
                    .with_source(e)
                })?;

                let as_init = actions::parse_runner_json(contents_str).map_err(|e| {
                    malformed_runner_error(format!("parsing runner.json for runner {}", self.id))
                        .with_source(e)
                })?;

                match as_init.validate() {
                    Ok(()) => {}
                    Err(e) => {
                        log_warn!(error:err = e, runner = self.id; "runner.json failed validation");

                        return Err(rt::errors::Error::vm(
                            genlayer_sdk::abi::consts::VmError::invalid_contract()
                                .runner()
                                .malformed(),
                        ));
                    }
                }

                Ok(Arc::new(as_init))
            })
            .await
            .map(Clone::clone)
    }

    pub fn get_file(&self, name: &str) -> rt::errors::Result<bytes::Bytes> {
        let contents = self
            .files
            .data
            .get(name)
            .ok_or_else(|| malformed_runner_error(format!("no file {name}")))
            .with_ctx(|| format!("reading runner {}", self.id))?;
        Ok(contents.clone())
    }
}

pub fn verify_runner(runner_id: &str) -> Option<(&str, &str)> {
    let (runner_id, runner_hash) = runner_id.split(":").collect_tuple()?;

    for c in runner_id.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' {
            log_warn!("character `{c}` is not allowed in runner id");

            return None;
        }
    }

    for c in runner_hash.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '=' {
            log_warn!("character `{c}` is not allowed in runner hash");

            return None;
        }
    }
    Some((runner_id, runner_hash))
}
