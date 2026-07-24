mod se;
pub use se::*;

mod de;
pub use de::*;

use crate::Value;

pub mod as_bytes;

pub trait HasDeserializer<'a>
where
    Self: 'a,
{
    type Deserializer: Deserializer + 'a;
    fn into_deserializer(self) -> Self::Deserializer;
}

impl<'a> HasDeserializer<'a> for &'a [u8] {
    type Deserializer = BinaryDeserializer<'a>;
    fn into_deserializer(self) -> Self::Deserializer {
        BinaryDeserializer::new(self)
    }
}

impl<'a> HasDeserializer<'a> for &'a Value {
    type Deserializer = RefValueDeserializer<'a>;
    fn into_deserializer(self) -> Self::Deserializer {
        RefValueDeserializer::new(self)
    }
}

impl HasDeserializer<'_> for Value {
    type Deserializer = ValueDeserializer;
    fn into_deserializer(self) -> Self::Deserializer {
        ValueDeserializer::new(self)
    }
}
