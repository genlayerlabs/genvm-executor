use crate::{Address, Value};
use mutatis::{Candidates, Context, DefaultMutate, Generate, Mutate, Result, mutators as m};
use num_bigint::{BigInt, Sign};

/// How deep a mutation may nest a value below the one it started from. Encoding
/// and decoding both recurse, and so does this mutator, so the bound keeps a
/// runaway value from overflowing the stack of the fuzzer itself.
const MAX_DEPTH: u8 = 3;

/// `Sign::NoSign` with a non-zero magnitude decodes back to zero, which would
/// silently undo the mutation that just changed those bytes.
fn with_sign(sign: Sign, magnitude: &[u8]) -> BigInt {
    let sign = match sign {
        Sign::NoSign => Sign::Plus,
        sign => sign,
    };
    BigInt::from_bytes_le(sign, magnitude)
}

/// Extends a string or byte string by a chunk rather than an element.
///
/// A length is encoded in more bytes as it grows, and growing one element per
/// mutation reaches the second length byte only after a hundred-odd lucky
/// draws. The chunk sizes step over those boundaries directly.
fn grow(mutations: &mut Candidates<'_>, mut extend: impl FnMut(usize)) -> Result<()> {
    const CHUNKS: [usize; 4] = [1, 127, 128, 1024];

    if mutations.shrink() {
        return Ok(());
    }
    mutations.mutation_group(CHUNKS.len() as u32, |_ctx, which| {
        extend(CHUNKS[which as usize]);
        Ok(())
    })
}

#[derive(Default)]
pub struct AddressMutator;

impl Mutate<Address> for AddressMutator {
    fn mutate(&mut self, mutations: &mut Candidates<'_>, value: &mut Address) -> Result<()> {
        mutations.mutation(|ctx| {
            let index = ctx.rng().gen_index(value.0.len()).unwrap_or(0);
            let mut byte = [0u8; 1];
            ctx.rng().gen_bytes(&mut byte);
            value.0[index] = byte[0];
            Ok(())
        })
    }
}

impl DefaultMutate for Address {
    type DefaultMutate = AddressMutator;
}

/// What a corpus file that no longer decodes falls back to, so a mutator always
/// has something to work from.
impl Default for Value {
    fn default() -> Self {
        Value::Null
    }
}

/// A [`Value`] as it is stored in a fuzz corpus.
///
/// [`Value`]'s own `serde` impls describe the value to the format -- a null is a
/// unit, a number is an `i64` when it fits -- which a self-describing format
/// reads back and a compact one cannot. The corpus needs the opposite: one
/// tagged shape that survives a round trip through `postcard`.
#[derive(Debug)]
pub struct Corpus(pub Value);

#[derive(serde::Serialize, serde::Deserialize)]
enum ValueRepr {
    Null,
    Address(Vec<u8>),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    Number { negative: bool, magnitude: Vec<u8> },
    Map(std::collections::BTreeMap<String, ValueRepr>),
    Array(Vec<ValueRepr>),
}

impl From<&Value> for ValueRepr {
    fn from(value: &Value) -> Self {
        match value {
            Value::Null => ValueRepr::Null,
            Value::Address(a) => ValueRepr::Address(a.0.to_vec()),
            Value::Bool(b) => ValueRepr::Bool(*b),
            Value::Str(s) => ValueRepr::Str(s.clone()),
            Value::Bytes(b) => ValueRepr::Bytes(b.clone()),
            Value::Number(n) => {
                let (sign, magnitude) = n.to_bytes_le();
                ValueRepr::Number {
                    negative: sign == Sign::Minus,
                    magnitude,
                }
            }
            Value::Map(m) => ValueRepr::Map(m.iter().map(|(k, v)| (k.clone(), v.into())).collect()),
            Value::Array(a) => ValueRepr::Array(a.iter().map(ValueRepr::from).collect()),
        }
    }
}

impl From<ValueRepr> for Value {
    fn from(repr: ValueRepr) -> Self {
        match repr {
            ValueRepr::Null => Value::Null,
            ValueRepr::Address(mut bytes) => {
                // The corpus carries a length the encoding does not have, so a
                // mutated one is normalized rather than rejected
                bytes.resize(Address::len(), 0);
                let mut raw = [0u8; crate::ADDRESS_SIZE];
                raw.copy_from_slice(&bytes);
                Value::Address(Address(raw))
            }
            ValueRepr::Bool(b) => Value::Bool(b),
            ValueRepr::Str(s) => Value::Str(s),
            ValueRepr::Bytes(b) => Value::Bytes(b),
            ValueRepr::Number {
                negative,
                magnitude,
            } => Value::Number(with_sign(
                if negative { Sign::Minus } else { Sign::Plus },
                &magnitude,
            )),
            ValueRepr::Map(m) => {
                Value::Map(m.into_iter().map(|(k, v)| (k, Value::from(v))).collect())
            }
            ValueRepr::Array(a) => Value::Array(a.into_iter().map(Value::from).collect()),
        }
    }
}

impl serde::Serialize for Corpus {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> core::result::Result<S::Ok, S::Error> {
        ValueRepr::from(&self.0).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Corpus {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> core::result::Result<Self, D::Error> {
        ValueRepr::deserialize(deserializer).map(|repr| Corpus(repr.into()))
    }
}

impl Default for Corpus {
    fn default() -> Self {
        Corpus(Value::Null)
    }
}

#[derive(Default)]
pub struct CorpusMutator(ValueMutator);

impl Mutate<Corpus> for CorpusMutator {
    fn mutate(&mut self, mutations: &mut Candidates<'_>, value: &mut Corpus) -> Result<()> {
        self.0.mutate(mutations, &mut value.0)
    }
}

impl DefaultMutate for Corpus {
    type DefaultMutate = CorpusMutator;
}

/// What a value looks like before anything has mutated it. A derived mutator
/// needs this to build the variant it is switching a containing enum to.
impl Generate<Corpus> for CorpusMutator {
    fn generate(&mut self, _context: &mut Context) -> Result<Corpus> {
        Ok(Corpus::default())
    }
}

pub struct ValueMutator {
    depth: u8,
}

impl Default for ValueMutator {
    fn default() -> Self {
        Self { depth: MAX_DEPTH }
    }
}

impl ValueMutator {
    fn nested(&self) -> Self {
        Self {
            depth: self.depth.saturating_sub(1),
        }
    }

    /// Registers the variants a switch may land on. `Array` and `Map` are the
    /// only ones that add depth, so they are offered while the depth budget
    /// lasts and withheld once it runs out -- a value that could never become a
    /// container would leave the whole nested half of the encoding unreachable.
    fn switch_variants(&mut self, mutations: &mut Candidates<'_>, value: &mut Value) -> Result<()> {
        // One candidate that picks a variant at random, rather than one per
        // variant: registering eight of them would make a wholesale switch
        // three times likelier than mutating the payload in place, and a value
        // that keeps getting replaced never grows interesting.
        let variants = if self.depth > 0 && !mutations.shrink() {
            8
        } else {
            6
        };
        mutations.mutation(|ctx| {
            *value = match ctx.rng().gen_index(variants).unwrap_or(0) {
                0 => Value::Null,
                1 => Value::Bool(false),
                2 => Value::Str(String::new()),
                3 => Value::Bytes(Vec::new()),
                4 => Value::Number(num_bigint::BigInt::from(0)),
                6 => Value::Array(Vec::new()),
                7 => Value::Map(crate::Map::new()),
                _ => {
                    let mut bytes = [0u8; Address::len()];
                    ctx.rng().gen_bytes(&mut bytes);
                    Value::Address(Address(bytes))
                }
            };
            Ok(())
        })
    }
}

impl Mutate<Value> for ValueMutator {
    fn mutate(&mut self, mutations: &mut Candidates<'_>, value: &mut Value) -> Result<()> {
        match value {
            Value::Null => {}
            Value::Bool(b) => m::default::<bool>().mutate(mutations, b)?,
            Value::Str(s) => {
                m::default::<String>().mutate(mutations, s)?;
                grow(mutations, |len| {
                    s.reserve(len);
                    s.extend(std::iter::repeat_n('a', len));
                })?;
            }
            Value::Bytes(b) => {
                m::default::<Vec<u8>>().mutate(mutations, b)?;
                grow(mutations, |len| b.resize(b.len() + len, 0))?;
            }
            Value::Address(a) => AddressMutator.mutate(mutations, a)?,
            Value::Number(n) => {
                // Through the magnitude bytes rather than an `i64`, so that
                // numbers past the 64 bit range stay reachable
                mutations.mutation(|ctx| {
                    let (sign, mut magnitude) = n.to_bytes_le();
                    let index = ctx.rng().gen_index(magnitude.len()).unwrap_or(0);
                    let mut byte = [0u8; 1];
                    ctx.rng().gen_bytes(&mut byte);
                    magnitude.resize(magnitude.len().max(index + 1), 0);
                    magnitude[index] = byte[0];
                    *n = with_sign(sign, &magnitude);
                    Ok(())
                })?;
                if !mutations.shrink() {
                    mutations.mutation(|ctx| {
                        let (sign, mut magnitude) = n.to_bytes_le();
                        let mut byte = [0u8; 1];
                        ctx.rng().gen_bytes(&mut byte);
                        magnitude.push(byte[0]);
                        *n = with_sign(sign, &magnitude);
                        Ok(())
                    })?;
                }
                mutations.mutation(|_ctx| {
                    *n = -std::mem::take(n);
                    Ok(())
                })?;
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    self.nested().mutate(mutations, item)?;
                }
                if self.depth > 0 && !mutations.shrink() {
                    mutations.mutation(|_ctx| {
                        items.push(Value::Null);
                        Ok(())
                    })?;
                }
                mutations.mutation(|_ctx| {
                    items.pop();
                    Ok(())
                })?;
            }
            Value::Map(map) => {
                for item in map.values_mut() {
                    self.nested().mutate(mutations, item)?;
                }
                if self.depth > 0 && !mutations.shrink() {
                    mutations.mutation(|ctx| {
                        let mut byte = [0u8; 1];
                        ctx.rng().gen_bytes(&mut byte);
                        map.insert(format!("k{}", byte[0]), Value::Null);
                        Ok(())
                    })?;
                }
                mutations.mutation(|ctx| {
                    if let Some(key) = ctx
                        .rng()
                        .gen_index(map.len())
                        .and_then(|index| map.keys().nth(index).cloned())
                    {
                        map.remove(&key);
                    }
                    Ok(())
                })?;
            }
        }

        self.switch_variants(mutations, value)
    }
}

impl DefaultMutate for Value {
    type DefaultMutate = ValueMutator;
}
