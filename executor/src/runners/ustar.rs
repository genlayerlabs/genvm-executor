use std::collections::BTreeMap;

use crate::rt::errors::{self, Error};

#[derive(Clone)]
pub struct LegacyArchive {
    pub data: BTreeMap<String, bytes::Bytes>,
    pub total_size: u32,
}

fn map_try_insert<K, V>(map: &mut BTreeMap<K, V>, key: K, value: V) -> errors::Result<&mut V>
where
    K: Ord + std::fmt::Display,
{
    use std::collections::btree_map::Entry::*;
    match map.entry(key) {
        Occupied(entry) => Err(Error::internal(format!(
            "entry {} is already occupied",
            entry.key()
        ))),
        Vacant(entry) => Ok(entry.insert(value)),
    }
}

fn trim_zeroes(x: &[u8]) -> &[u8] {
    let mut idx = x.len() - 1;
    while idx > 0 && x[idx - 1] == 0 {
        idx -= 1;
    }

    &x[0..idx]
}

impl LegacyArchive {
    pub fn from_ustar(original_data: bytes::Bytes) -> errors::Result<Self> {
        const BLOCK_SIZE: usize = 512;
        const _RECORD_SIZE: usize = BLOCK_SIZE * 20;

        if original_data.len() < BLOCK_SIZE * 2 {
            return Err(Error::internal("archive is too short for tar"));
        }

        if !original_data.len().is_multiple_of(BLOCK_SIZE) {
            return Err(Error::internal("tar len % 512 != 0"));
        }

        let mut res = BTreeMap::new();

        let mut begin = 0;
        while begin + 2 * BLOCK_SIZE <= original_data.len() {
            let data = original_data.slice(begin..original_data.len());
            let header = data.slice(0..BLOCK_SIZE);

            if data.slice(0..BLOCK_SIZE * 2).iter().all(|x| *x == 0) {
                break;
            }

            let header_signature = &header[257..265];

            if header_signature != b"ustar\x0000" {
                return Err(Error::internal(format!(
                    "invalid ustar header={header_signature:?}; offset={begin}"
                )));
            }

            let file_size_octal = trim_zeroes(&header[124..136]);

            let link_indicator = header[156];
            if ![b'0', b'\x00', b'5'].contains(&link_indicator) {
                return Err(Error::internal("links are forbidden"));
            }

            let path_and_name = trim_zeroes(&header[0..100]);
            let path_and_name_prefix = trim_zeroes(&header[345..345 + 155]);

            begin += BLOCK_SIZE;

            let mut name_vec = Vec::from(path_and_name_prefix);
            name_vec.extend_from_slice(path_and_name);

            let name = String::from_utf8(name_vec)?;

            if name.ends_with("/") {
                continue;
            }

            let mut file_size = 0_usize;
            for c in file_size_octal.iter().cloned() {
                if !(b'0'..=b'7').contains(&c) {
                    return Err(Error::internal(format!("invalid octal ascii {c}")));
                }
                file_size = file_size
                    .checked_mul(8)
                    .and_then(|v| v.checked_add((c - b'0') as usize))
                    .ok_or_else(|| Error::internal("tar file size overflow"))?;
            }

            begin += file_size;
            begin += (BLOCK_SIZE - (begin % BLOCK_SIZE)) % BLOCK_SIZE;

            let file_end = BLOCK_SIZE
                .checked_add(file_size)
                .filter(|end| *end <= data.len())
                .ok_or_else(|| Error::internal("tar file size exceeds archive bounds"))?;
            let file_contents = data.slice(BLOCK_SIZE..file_end);

            map_try_insert(&mut res, name, file_contents)?;
        }

        let res = Self {
            data: res,
            total_size: original_data.len() as u32,
        };
        // Files are carved from non-overlapping, header/padding-separated slices
        // of the tar, so their contents sum to at most the whole buffer -- the
        // amount `total_size` charges.
        #[cfg(debug_assertions)]
        {
            let content_sum: u64 = res.data.values().map(|v| v.len() as u64).sum();
            debug_assert!(
                content_sum <= original_data.len() as u64,
                "ustar file contents {content_sum} exceed archive size {}",
                original_data.len(),
            );
        }
        Ok(res)
    }
}
