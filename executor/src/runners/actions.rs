use std::{collections::BTreeMap, sync::Arc};

use genlayer_sdk::abi;
use genvm_common::internal_constants::runner_limits;

use crate::int_traits::*;
use crate::rt;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum WasmMode {
    Det,
    #[serde(rename = "!det")]
    Nondet,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(transparent)]
#[derive(Default)]
pub struct SetArgs(pub Vec<String>);

impl SetArgs {
    pub fn validate(&self) -> core::result::Result<(), InitActionError> {
        for item in &self.0 {
            validate_string(item)?;
        }
        Ok(())
    }
}

pub struct Env {
    limiter: rt::memlimiter::Limiter,
    pub vars: BTreeMap<String, String>,
}

impl Env {
    pub fn new(limiter: rt::memlimiter::Limiter) -> Self {
        Self {
            limiter,
            vars: BTreeMap::new(),
        }
    }

    pub fn set_patching(
        &mut self,
        name: &str,
        val: &str,
    ) -> core::result::Result<(), rt::errors::Error> {
        if name.len() > runner_limits::ENV_NAME_LEN.into_int_comptime() {
            return Err(rt::errors::Error::vm_cause(
                abi::consts::VmError::invalid_contract()
                    .runner()
                    .malformed(),
                Some(anyhow::anyhow!(
                    "env var name is too long: {} bytes",
                    name.len()
                )),
            ));
        }

        for c in name.chars() {
            if !c.is_ascii() || c == '=' || c.is_control() || c.is_whitespace() {
                return Err(rt::errors::Error::vm_cause(
                    abi::consts::VmError::invalid_contract()
                        .runner()
                        .malformed(),
                    Some(anyhow::anyhow!(
                        "env var name contains invalid character: `{c:?}`"
                    )),
                ));
            }
        }

        let new_val = match genvm_common::templater::patch_str(
            &self.vars,
            val,
            &genvm_common::templater::DOLLAR_UNFOLDER_RE,
        ) {
            Ok(v) => v,
            Err(e) => {
                return Err(rt::errors::Error::vm_cause(
                    abi::consts::VmError::invalid_contract()
                        .runner()
                        .malformed(),
                    Some(e),
                ));
            }
        };

        if new_val.len() > runner_limits::ENV_VALUE_LEN.into_int_comptime() {
            return Err(rt::errors::Error::vm_cause(
                abi::consts::VmError::invalid_contract()
                    .runner()
                    .malformed(),
                Some(anyhow::anyhow!(
                    "env var {name:?} value is too long: {} bytes",
                    new_val.len()
                )),
            ));
        }

        if !self
            .limiter
            .consume(new_val.len().into_int_downcast_panicking())
        {
            return Err(rt::errors::Error::vm_cause(
                abi::consts::VmError::out_of().memory().val(),
                Some(anyhow::anyhow!(
                    "env var {name:?} value is too long: {} bytes, exceeds memory limit",
                    new_val.len()
                )),
            ));
        }

        for c in new_val.chars() {
            if c == '\0' {
                return Err(rt::errors::Error::vm_cause(
                    abi::consts::VmError::invalid_contract()
                        .runner()
                        .malformed(),
                    Some(anyhow::anyhow!(
                        "env var {name:?} value contains invalid character: `{c:?}`"
                    )),
                ));
            }
        }

        self.vars.insert(name.to_string(), new_val);
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum InitAction {
    MapFile {
        to: Arc<str>,
        file: Arc<str>,
    },
    AddEnv {
        name: String,
        val: String,
    },
    SetArgs(SetArgs),
    Depends(String),
    LinkWasm(Arc<str>),
    StartWasm(Arc<str>),

    When {
        cond: WasmMode,
        action: Box<InitAction>,
    },
    Seq(Vec<InitAction>),

    With {
        runner: String,
        action: Box<InitAction>,
    },
}

struct RunnerJson(InitAction);

/// Hides top-level `$schema` annotations so the rest of the object is
/// deserialized by `InitAction`'s derived impl rather than a second hand-written
/// dispatch. The underlying map is forwarded as a stream, not collapsed into a
/// `Value` first, which is what keeps serde's duplicate-field detection working.
struct SkipSchema<M> {
    inner: M,
    seen_schema: bool,
}

impl<M> SkipSchema<M> {
    fn skipping_key_seed<'de, K>(&mut self, seed: K) -> Result<Option<K::Value>, M::Error>
    where
        M: serde::de::MapAccess<'de>,
        K: serde::de::DeserializeSeed<'de>,
    {
        loop {
            let Some(key) = self.inner.next_key::<String>()? else {
                return Ok(None);
            };
            if key != "$schema" {
                return seed
                    .deserialize(serde::de::value::StrDeserializer::<M::Error>::new(&key))
                    .map(Some);
            }
            if self.seen_schema {
                return Err(serde::de::Error::duplicate_field("$schema"));
            }
            self.seen_schema = true;
            self.inner.next_value::<serde::de::IgnoredAny>()?;
        }
    }
}

impl<'de, M> serde::de::MapAccess<'de> for SkipSchema<M>
where
    M: serde::de::MapAccess<'de>,
{
    type Error = M::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: serde::de::DeserializeSeed<'de>,
    {
        self.skipping_key_seed(seed)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        self.inner.next_value_seed(seed)
    }
}

impl<'de> serde::Deserialize<'de> for RunnerJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RunnerJson;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a runner.json object")
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut map = SkipSchema {
                    inner: map,
                    seen_schema: false,
                };
                // The enum consumes exactly its own key, so anything left is
                // either a trailing `$schema` (swallowed by `SkipSchema`) or a
                // second action, which no runner.json may carry.
                let action = <InitAction as serde::Deserialize>::deserialize(
                    serde::de::value::MapAccessDeserializer::new(&mut map),
                )?;
                if let Some(field) = serde::de::MapAccess::next_key::<String>(&mut map)? {
                    return Err(serde::de::Error::custom(format_args!(
                        "unexpected field `{field}`"
                    )));
                }
                Ok(RunnerJson(action))
            }
        }

        deserializer.deserialize_map(Visitor)
    }
}

pub(super) fn parse_runner_json(contents: &str) -> serde_json::Result<InitAction> {
    serde_json::from_str(contents).map(|RunnerJson(action)| action)
}

#[derive(thiserror::Error, Debug)]
pub enum InitActionError {
    #[error("nul byte in string")]
    NullInString,
    #[error("depth limit exceeded")]
    DepthLimitExceeded,
}

fn validate_string(s: &str) -> core::result::Result<(), InitActionError> {
    if s.contains('\0') {
        return Err(InitActionError::NullInString);
    }
    Ok(())
}

impl InitAction {
    pub fn validate(&self) -> core::result::Result<(), InitActionError> {
        self.validate_impl(0)
    }

    fn validate_impl(&self, depth: usize) -> core::result::Result<(), InitActionError> {
        if depth >= runner_limits::INIT_ACTION_DEPTH.into_int_comptime() {
            return Err(InitActionError::DepthLimitExceeded);
        }

        match self {
            InitAction::MapFile { to, file } => {
                validate_string(to)?;
                validate_string(file)?;
            }
            InitAction::AddEnv { name, val } => {
                validate_string(name)?;
                validate_string(val)?;
            }
            InitAction::SetArgs(items) => {
                items.validate()?;
            }
            InitAction::Depends(s) => {
                validate_string(s)?;
            }
            InitAction::LinkWasm(s) => {
                validate_string(s)?;
            }
            InitAction::StartWasm(s) => {
                validate_string(s)?;
            }
            InitAction::When { cond: _, action } => {
                action.validate_impl(depth + 1)?;
            }
            InitAction::Seq(init_actions) => {
                for action in init_actions {
                    action.validate_impl(depth + 1)?;
                }
            }
            InitAction::With { runner, action } => {
                validate_string(runner)?;
                action.validate_impl(depth + 1)?;
            }
        }

        Ok(())
    }
}
