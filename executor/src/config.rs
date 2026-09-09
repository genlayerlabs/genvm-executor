use serde_derive::Deserialize;

#[derive(Deserialize)]
pub struct Module {
    pub address: String,
}

#[derive(Deserialize)]
pub struct Modules {
    pub llm: Module,
    pub web: Module,
}

fn default_fee_expr_zero() -> String {
    "0".to_owned()
}

fn deserialize_bucket_names<'de, D>(d: D) -> Result<Vec<symbol_table::GlobalSymbol>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct Visitor;
    impl<'de> de::Visitor<'de> for Visitor {
        type Value = Vec<symbol_table::GlobalSymbol>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a non-empty string or array of non-empty strings")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.is_empty() {
                return Err(E::custom("bucket name must not be empty"));
            }
            Ok(vec![symbol_table::GlobalSymbol::from(v)])
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(name) = seq.next_element::<String>()? {
                if name.is_empty() {
                    return Err(de::Error::custom("bucket name must not be empty"));
                }
                v.push(symbol_table::GlobalSymbol::from(name));
            }
            if v.is_empty() {
                return Err(de::Error::custom("buckets must have at least one entry"));
            }
            Ok(v)
        }
    }
    d.deserialize_any(Visitor)
}

#[derive(Clone, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct FeesBucketConfig {
    #[serde(deserialize_with = "deserialize_bucket_names")]
    pub buckets: Vec<symbol_table::GlobalSymbol>,
    /// Cost charged once, up-front, when the bucket is created
    /// (the fixed part of `start + sum of per-change`).
    #[serde(default = "default_fee_expr_zero")]
    pub subtract_on_start_expr: String,
    /// Cost charged per change, evaluated with the `units` variable.
    pub delta_expr: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct FeesConfig {
    pub expr_prelude: String,
    pub storage: FeesBucketConfig,
    pub message_receipt: FeesBucketConfig,
    pub nondet_output: FeesBucketConfig,
    pub message_fee: FeesBucketConfig,
    pub event: FeesBucketConfig,
}

#[derive(Deserialize)]
pub struct Config {
    pub modules: Modules,
    pub fees: FeesConfig,
    pub cache_dir: String,
    pub runners_dir: String,
    pub registry_dir: String,

    #[serde(flatten)]
    pub base: genvm_common::BaseConfig,
}
