use crate::public_abi::top_limits;
use bytes::Bytes;

#[derive(Debug, PartialEq, Eq, genlayer_calldata::Encode)]
pub struct LeaderPublicData {
    pub nd_outs: Vec<Bytes>,
}

impl genlayer_calldata::codec::Decode for LeaderPublicData {
    fn decode<D: genlayer_calldata::codec::Deserializer>(
        deserializer: D,
    ) -> Result<Self, genlayer_calldata::codec::DecodeError> {
        use genlayer_calldata::codec::{DecodeError, MapAccess, SeqAccess, Visitor};

        struct OutputsVisitor;
        impl Visitor for OutputsVisitor {
            type Value = Vec<Bytes>;

            fn visit_seq<A: SeqAccess>(
                self,
                len: u64,
                mut seq: A,
            ) -> Result<Self::Value, DecodeError> {
                if len > u64::from(top_limits::NONDET_BLOCKS) {
                    return Err(DecodeError::Custom(
                        "too many nondeterministic outputs".to_owned(),
                    ));
                }

                let mut outputs = Vec::with_capacity(len as usize);
                while let Some(output) = seq.next_element::<Bytes>()? {
                    outputs.push(output);
                }
                debug_assert_eq!(outputs.len(), len as usize);
                Ok(outputs)
            }
        }

        struct LeaderPublicDataVisitor;
        impl Visitor for LeaderPublicDataVisitor {
            type Value = LeaderPublicData;

            fn visit_map<A: MapAccess>(
                self,
                len: u64,
                mut map: A,
            ) -> Result<Self::Value, DecodeError> {
                if len != 1 {
                    return Err(DecodeError::LengthMismatch {
                        expected: 1,
                        got: usize::try_from(len).unwrap_or(usize::MAX),
                    });
                }
                let Some(key) = map.next_key()? else {
                    return Err(DecodeError::FieldMissing("nd_outs"));
                };
                if key != "nd_outs" {
                    return Err(DecodeError::UnknownField(key.to_owned()));
                }

                let nd_outs = map.next_value_visit(OutputsVisitor)?;
                debug_assert!(map.next_key()?.is_none());
                Ok(LeaderPublicData { nd_outs })
            }
        }

        deserializer.deserialize(LeaderPublicDataVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

impl LeaderPublicData {
    pub fn encode(&self) -> Bytes {
        genlayer_calldata::encode_obj(self).into()
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, DecodeError> {
        genlayer_calldata::decode_obj(encoded).map_err(|_| DecodeError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calldata_round_trip() {
        let data = LeaderPublicData {
            nd_outs: vec![Bytes::from_static(b"a"), Bytes::from_static(b"bc")],
        };

        assert_eq!(LeaderPublicData::decode(&data.encode()), Ok(data));
    }

    #[test]
    fn has_stable_calldata_encoding() {
        let data = LeaderPublicData {
            nd_outs: vec![Bytes::from_static(b"a"), Bytes::from_static(b"bc")],
        };

        assert_eq!(data.encode().as_ref(), b"\x0e\x07nd_outs\x15\x0ba\x13bc");
    }

    #[test]
    fn rejects_empty_legacy_and_trailing_data() {
        assert_eq!(LeaderPublicData::decode(&[]), Err(DecodeError));
        assert_eq!(
            LeaderPublicData::decode(b"\xcc\x84test\x86padded"),
            Err(DecodeError)
        );

        let mut encoded = LeaderPublicData {
            nd_outs: Vec::new(),
        }
        .encode()
        .to_vec();
        encoded.push(0);
        assert_eq!(LeaderPublicData::decode(&encoded), Err(DecodeError));
    }

    #[test]
    fn bounds_output_count_while_decoding() {
        let at_limit = LeaderPublicData {
            nd_outs: vec![Bytes::new(); top_limits::NONDET_BLOCKS as usize],
        };
        assert_eq!(LeaderPublicData::decode(&at_limit.encode()), Ok(at_limit));

        let above_limit = LeaderPublicData {
            nd_outs: vec![Bytes::new(); top_limits::NONDET_BLOCKS as usize + 1],
        };
        assert_eq!(
            LeaderPublicData::decode(&above_limit.encode()),
            Err(DecodeError)
        );
    }
}
