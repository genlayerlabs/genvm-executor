// Vendored from the `mt19937` crate 3.1.0 (https://github.com/RustPython/mt19937),
// trimmed to what the executor uses; `fill_bytes` yields the same bytes as its
// `rand_core` 0.9 `RngCore` impl. Contract-visible output: keep it bit-exact.
//
// mt19937ar.c header:
//
// A C-program for MT19937, with initialization improved 2002/1/26.
// Coded by Takuji Nishimura and Makoto Matsumoto.
//
// Copyright (C) 1997 - 2002, Makoto Matsumoto and Takuji Nishimura,
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//   1. Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in the
//      documentation and/or other materials provided with the distribution.
//
//   3. The names of its contributors may not be used to endorse or promote
//      products derived from this software without specific prior written
//      permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT OWNER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
// LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
// NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908b0df;
const UPPER_MASK: u32 = 0x80000000;
const LOWER_MASK: u32 = 0x7fffffff;

/// MT19937 Mersenne Twister.
pub struct MT19937 {
    mt: [u32; N],
    mti: usize,
}

impl MT19937 {
    /// `init_by_array` of mt19937ar.c.
    pub fn new_with_slice_seed(init_key: &[u32]) -> Self {
        let mut state = Self { mt: [0; N], mti: N };
        state.seed_slice(init_key);
        state
    }

    fn seed(&mut self, s: u32) {
        self.mt[0] = s;
        self.mti = 1;
        while self.mti < N {
            self.mt[self.mti] = 1812433253u32
                .wrapping_mul(self.mt[self.mti - 1] ^ (self.mt[self.mti - 1] >> 30))
                .wrapping_add(self.mti as u32);
            self.mti += 1;
        }
    }

    fn seed_slice(&mut self, init_key: &[u32]) {
        self.seed(19650218);
        let mut i = 1;
        let mut j = 0;
        let mut k = N.max(init_key.len());
        while k != 0 {
            self.mt[i] = (self.mt[i]
                ^ ((self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1664525u32)))
            .wrapping_add(init_key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= init_key.len() {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k != 0 {
            self.mt[i] = (self.mt[i]
                ^ ((self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1566083941u32)))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }

        self.mt[0] = 0x80000000;
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut y: u32;
        let mag01 = |x| if (x & 0x1) == 1 { MATRIX_A } else { 0 };

        if self.mti >= N {
            for kk in 0..N - M {
                y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ mag01(y);
            }
            for kk in N - M..N - 1 {
                y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M - N] ^ (y >> 1) ^ mag01(y);
            }
            y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
            self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ mag01(y);

            self.mti = 0;
        }

        y = self.mt[self.mti];
        self.mti += 1;

        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;

        y
    }

    /// Little-endian words; a trailing partial word consumes a whole output.
    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(4) {
            chunk.copy_from_slice(&self.next_u32().to_le_bytes()[..chunk.len()]);
        }
    }
}
