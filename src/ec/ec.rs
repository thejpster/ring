// Copyright 2015 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

use {c, init, rand};
use untrusted;

/// A key agreement algorithm.
#[cfg_attr(not(test), allow(dead_code))]
pub struct AgreementAlgorithmImpl {
    pub public_key_len: usize,
    pub elem_and_scalar_len: usize,

    pub nid: c::int,

    generate_private_key:
        fn(rng: &rand::SecureRandom) -> Result<PrivateKey, ()>,

    public_from_private:
        fn(public_out: &mut [u8], private_key: &PrivateKey) -> Result<(), ()>,

    pub ecdh:
        fn(out: &mut [u8], private_key: &PrivateKey,
           peer_public_key: untrusted::Input) -> Result<(), ()>,
}

pub struct PrivateKey {
    bytes: [u8; SCALAR_MAX_BYTES],
}

impl <'a> PrivateKey {
    pub fn generate(alg: &AgreementAlgorithmImpl, rng: &rand::SecureRandom)
                    -> Result<PrivateKey, ()> {
        init::init_once();
        (alg.generate_private_key)(rng)
    }

    #[cfg(test)]
    pub fn from_test_vector(alg: &AgreementAlgorithmImpl, test_vector: &[u8])
                            -> PrivateKey {
        init::init_once();
        let mut result = PrivateKey {
            bytes: [0; SCALAR_MAX_BYTES],
        };
        {
            let private_key_bytes =
                &mut result.bytes[..alg.elem_and_scalar_len];
            assert_eq!(test_vector.len(), private_key_bytes.len());
            for i in 0..private_key_bytes.len() {
                private_key_bytes[i] = test_vector[i];
            }
        }
        result
    }

    #[cfg(test)]
    pub fn bytes(&'a self) -> &'a [u8] {
        &self.bytes[..]
    }

    #[inline(always)]
    pub fn compute_public_key(&self, alg: &AgreementAlgorithmImpl,
                              out: &mut [u8]) -> Result<(), ()> {
        if out.len() != alg.public_key_len {
            return Err(());
        }
        (alg.public_from_private)(out, &self)
    }
}


const ELEM_MAX_BITS: usize = 384;
pub const ELEM_MAX_BYTES: usize = (ELEM_MAX_BITS + 7) / 8;

const SCALAR_MAX_BYTES: usize = ELEM_MAX_BYTES;

/// The maximum length, in bytes, of an encoded public key..
pub const PUBLIC_KEY_MAX_LEN: usize = 1 + (2 * ELEM_MAX_BYTES);


pub mod eddsa;

#[path = "suite_b/suite_b.rs"]
pub mod suite_b;

pub mod x25519;
