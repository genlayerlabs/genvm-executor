use genvm_common::util::mt19937::MT19937;

/// The executor seeds from a 32-byte digest read as 8 little-endian words.
fn digest_key(digest: [u8; 32]) -> [u32; 8] {
    std::array::from_fn(|i| u32::from_le_bytes(digest[i * 4..i * 4 + 4].try_into().unwrap()))
}

fn counting_key() -> [u32; 8] {
    digest_key(std::array::from_fn(|i| i as u8))
}

/// Words 0..4 and, past the first state regeneration, words 624..628.
fn assert_words(key: &[u32], first: [u32; 4], after_regen: [u32; 4]) {
    let mut rng = MT19937::new_with_slice_seed(key);
    assert_eq!(std::array::from_fn(|_| rng.next_u32()), first);
    for _ in 0..620 {
        rng.next_u32();
    }
    assert_eq!(std::array::from_fn(|_| rng.next_u32()), after_regen);
}

fn fills(key: &[u32], lens: &[usize]) -> (Vec<Vec<u8>>, u32) {
    let mut rng = MT19937::new_with_slice_seed(key);
    let out = lens
        .iter()
        .map(|&len| {
            let mut buf = vec![0; len];
            rng.fill_bytes(&mut buf);
            buf
        })
        .collect();
    (out, rng.next_u32())
}

// -- Golden words, produced by the mt19937 3.1.0 crate --

#[test]
fn single_word_seed() {
    assert_words(
        &[12345],
        [0x6aa79987, 0xbb91433a, 0x029a7245, 0xd1f6f86c],
        [0xf8a76ffd, 0xd9820ed4, 0xb3b2cbcd, 0x7b8837d1],
    );
    assert_words(
        &[0],
        [0xd82c07cd, 0x629f6fbe, 0xc2094cac, 0xe3e70682],
        [0x84dd6da6, 0x4a31b243, 0x72d3cc5d, 0xeb8f2056],
    );
}

#[test]
fn digest_seeds() {
    assert_words(
        &digest_key([0; 32]),
        [0xabf1a729, 0x7723f9d0, 0xfc9a81c6, 0x93d7d07a],
        [0xacc03180, 0xb916d191, 0x1fd69926, 0x8342b4d2],
    );
    assert_words(
        &counting_key(),
        [0x0298a4a0, 0xf309e1ee, 0xc893e1e6, 0xe67e6db0],
        [0xc0726918, 0x568cb24b, 0xc291298e, 0x3fbea125],
    );
    assert_words(
        &digest_key([0xff; 32]),
        [0x7a2d3651, 0xd4b3c0ca, 0x1262f77a, 0x1a0836db],
        [0x666710cc, 0x54bc7525, 0x03cc8d6a, 0xec38e711],
    );
}

#[test]
fn key_longer_than_state() {
    let key: Vec<u32> = (0..700).collect();
    assert_words(
        &key,
        [0xde2e96c0, 0x72216bbc, 0xea35d44f, 0xd6ca0756],
        [0x42da6835, 0x48a3b2b9, 0x7850fd22, 0x18510bc9],
    );
}

// -- fill_bytes layout --

#[test]
fn fill_bytes_is_little_endian_words() {
    let (out, next) = fills(&counting_key(), &[20]);
    assert_eq!(
        out,
        [[
            0xa0, 0xa4, 0x98, 0x02, 0xee, 0xe1, 0x09, 0xf3, 0xe6, 0xe1, 0x93, 0xc8, 0xb0, 0x6d,
            0x7e, 0xe6, 0xa7, 0x17, 0xa4, 0xd7,
        ]]
    );
    assert_eq!(next, 0xd9c120b8);
}

#[test]
fn fill_bytes_partial_word_discards_remainder() {
    let (out, next) = fills(&counting_key(), &[1, 2, 3, 5, 6, 7, 9]);
    let expected: [&[u8]; 7] = [
        &[160],
        &[238, 225],
        &[230, 225, 147],
        &[176, 109, 126, 230, 167],
        &[184, 32, 193, 217, 177, 115],
        &[194, 71, 182, 129, 121, 9, 140],
        &[96, 137, 16, 143, 215, 157, 110, 132, 43],
    ];
    assert_eq!(out, expected);
    assert_eq!(next, 0x7f33dca9);
}

#[test]
fn fill_bytes_empty_consumes_nothing() {
    let (out, next) = fills(&counting_key(), &[0, 4, 0, 3]);
    let expected: [&[u8]; 4] = [&[], &[160, 164, 152, 2], &[], &[238, 225, 9]];
    assert_eq!(out, expected);
    assert_eq!(next, 0xc893e1e6);
}
