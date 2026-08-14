use genlayer_calldata as calldata;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let Some(calldata::fuzzing::Corpus(generated)) = genvm_fuzzing::decode(data) else {
            return;
        };

        let encoded = calldata::encode(&generated);
        let decoded = calldata::decode(&encoded).unwrap();

        assert_eq!(generated, decoded);

        let encoded_with_codec = {
            let mut buf = Vec::new();
            calldata::codec::Encode::encode(&generated, &mut calldata::Encoder::new(&mut buf))
                .unwrap();
            buf
        };
        assert_eq!(encoded, encoded_with_codec, "Codec encoding mismatch");
        let decoded_with_codec = calldata::codec::Decode::decode(
            calldata::codec::BinaryDeserializer::new(&encoded_with_codec),
        )
        .unwrap();
        assert_eq!(decoded, decoded_with_codec, "Codec decoding mismatch");
    });
}
