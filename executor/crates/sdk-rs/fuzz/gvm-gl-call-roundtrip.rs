#![allow(deprecated)]

use arbitrary::Arbitrary;
use genlayer_calldata::{Encoder, codec};
use genlayer_sdk::abi::gl_call::Message;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let mut u = arbitrary::Unstructured::new(data);
        let msg = match Message::arbitrary(&mut u) {
            Ok(m) => m,
            Err(_) => return,
        };

        // Encode via calldata codec
        let mut buf = Vec::new();
        codec::Encode::encode(&msg, &mut Encoder::new(&mut buf)).unwrap();

        // Decode binary back to Value
        let decoded_value = genlayer_calldata::decode(&buf).unwrap();

        // Re-encode the decoded Value to binary
        let reencoded = genlayer_calldata::encode(&decoded_value);

        // Binary roundtrip must be identical
        assert_eq!(buf, reencoded, "binary roundtrip mismatch");

        // Decode back to Message via calldata codec
        let msg_decoded: Message =
            codec::Decode::decode(codec::ValueDeserializer(decoded_value)).unwrap();

        assert_eq!(msg, msg_decoded, "Message roundtrip mismatch: value");

        let msg_decoded_from_binary: Message =
            codec::Decode::decode(codec::BinaryDeserializer::new(&buf)).unwrap();

        // A byte-backed source defers every `Maybe` field as raw bytes, while
        // `msg` holds them materialized, and `Maybe` compares the two
        // representations unequal. Re-encoding must therefore be the first
        // comparison: it is the one that holds whatever each side deferred.
        let mut reencoded_from_binary = Vec::new();
        codec::Encode::encode(
            &msg_decoded_from_binary,
            &mut Encoder::new(&mut reencoded_from_binary),
        )
        .unwrap();

        assert_eq!(
            buf, reencoded_from_binary,
            "Message roundtrip mismatch: binary bytes"
        );

        // Decoding those bytes through a `Value` -- which has nothing to defer --
        // materializes every deferred field at once, so the messages can be
        // compared without naming the fields that happen to be deferrable today.
        let msg_materialized: Message = codec::Decode::decode(codec::ValueDeserializer(
            genlayer_calldata::decode(&reencoded_from_binary).unwrap(),
        ))
        .unwrap();

        assert_eq!(msg, msg_materialized, "Message roundtrip mismatch: binary");
    });
}
