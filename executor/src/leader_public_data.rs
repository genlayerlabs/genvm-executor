use bytes::Bytes;

const PADDING: &[u8] = b"padded";

#[derive(Debug, PartialEq, Eq)]
pub struct LeaderPublicData {
    pub nondet_block_outputs: Vec<Bytes>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

impl LeaderPublicData {
    pub fn encode(&self) -> Bytes {
        let mut payload = Vec::new();
        for output in self
            .nondet_block_outputs
            .iter()
            .map(Bytes::as_ref)
            .chain(std::iter::once(PADDING))
        {
            encode_bytes(&mut payload, output);
        }

        let mut encoded = Vec::new();
        encode_len(&mut encoded, payload.len(), 0xc0, 0xf7);
        encoded.extend_from_slice(&payload);
        encoded.into()
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, DecodeError> {
        if encoded.is_empty() {
            return Ok(Self {
                nondet_block_outputs: Vec::new(),
            });
        }

        let (payload_start, payload_len) = decode_len(encoded, 0, true)?;
        let payload_end = payload_start.checked_add(payload_len).ok_or(DecodeError)?;
        if payload_end != encoded.len() {
            return Err(DecodeError);
        }

        let mut cursor = payload_start;
        let mut outputs = Vec::new();
        while cursor < payload_end {
            let (data_start, data_len) = decode_len(encoded, cursor, false)?;
            let data_end = data_start.checked_add(data_len).ok_or(DecodeError)?;
            if data_end > payload_end {
                return Err(DecodeError);
            }
            outputs.push(Bytes::copy_from_slice(&encoded[data_start..data_end]));
            cursor = data_end;
        }

        if outputs.last().is_none_or(|last| last.as_ref() != PADDING) {
            return Err(DecodeError);
        }
        outputs.pop();

        Ok(Self {
            nondet_block_outputs: outputs,
        })
    }
}

fn encode_bytes(output: &mut Vec<u8>, value: &[u8]) {
    if value.len() == 1 && value[0] < 0x80 {
        output.push(value[0]);
        return;
    }

    encode_len(output, value.len(), 0x80, 0xb7);
    output.extend_from_slice(value);
}

fn encode_len(output: &mut Vec<u8>, len: usize, short_base: u8, long_base: u8) {
    if len <= 55 {
        output.push(short_base + len as u8);
        return;
    }

    let bytes = len.to_be_bytes();
    let first = bytes.iter().position(|byte| *byte != 0).unwrap();
    let len_bytes = &bytes[first..];
    output.push(long_base + len_bytes.len() as u8);
    output.extend_from_slice(len_bytes);
}

fn decode_len(encoded: &[u8], offset: usize, list: bool) -> Result<(usize, usize), DecodeError> {
    let prefix = *encoded.get(offset).ok_or(DecodeError)?;
    let short_base: u8 = if list { 0xc0 } else { 0x80 };
    let long_base: u8 = if list { 0xf7 } else { 0xb7 };

    if !list && prefix < 0x80 {
        return Ok((offset, 1));
    }
    if prefix < short_base || prefix > long_base.saturating_add(size_of::<usize>() as u8) {
        return Err(DecodeError);
    }
    if prefix <= long_base {
        if !list && prefix == 0x81 && encoded.get(offset + 1).is_some_and(|byte| *byte < 0x80) {
            return Err(DecodeError);
        }
        return Ok((offset + 1, usize::from(prefix - short_base)));
    }

    let len_len = usize::from(prefix - long_base);
    let len_start = offset.checked_add(1).ok_or(DecodeError)?;
    let len_end = len_start.checked_add(len_len).ok_or(DecodeError)?;
    let len_bytes = encoded.get(len_start..len_end).ok_or(DecodeError)?;
    if len_bytes.first() == Some(&0) {
        return Err(DecodeError);
    }

    let mut buf = [0; size_of::<usize>()];
    buf[size_of::<usize>() - len_len..].copy_from_slice(len_bytes);
    let len = usize::from_be_bytes(buf);
    if len <= 55 {
        return Err(DecodeError);
    }

    Ok((len_end, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rlp_round_trip() {
        let data = LeaderPublicData {
            nondet_block_outputs: vec![Bytes::from_static(b"a"), Bytes::from_static(b"bc")],
        };

        assert_eq!(LeaderPublicData::decode(&data.encode()), Ok(data));
    }

    #[test]
    fn preserves_legacy_encoding() {
        let data = LeaderPublicData {
            nondet_block_outputs: vec![Bytes::from_static(b"test")],
        };

        assert_eq!(data.encode().as_ref(), b"\xcc\x84test\x86padded");
    }

    #[test]
    fn empty_timeout_decodes_as_no_outputs() {
        assert_eq!(
            LeaderPublicData::decode(&[]),
            Ok(LeaderPublicData {
                nondet_block_outputs: Vec::new()
            })
        );
    }

    #[test]
    fn rejects_noncanonical_rlp() {
        assert_eq!(LeaderPublicData::decode(b"\xc0"), Err(DecodeError));
        assert_eq!(
            LeaderPublicData::decode(b"\xc7\x86padded\x00"),
            Err(DecodeError)
        );
        assert_eq!(LeaderPublicData::decode(b"\xc2\x81\x01"), Err(DecodeError));
    }
}
