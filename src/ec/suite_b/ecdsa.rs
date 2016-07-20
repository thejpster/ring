// Copyright 2015-2016 Brian Smith.
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

//! ECDSA Signatures using the P-256 and P-384 curves.

use {der, digest, signature, signature_impl};
use super::verify_jacobian_point_is_on_the_curve;
use super::ops::*;
use super::public_key::*;
use untrusted;

struct ECDSAVerification {
    ops: &'static PublicScalarOps,
    digest_alg: &'static digest::Algorithm,
}

impl signature_impl::VerificationAlgorithmImpl for ECDSAVerification {
    // Verify an ECDSA signature as documented in the NSA Suite B Implementer's
    // Guide to ECDSA Section 3.4.2: ECDSA Signature Verification.
    fn verify(&self, public_key: untrusted::Input, msg: untrusted::Input,
              signature: untrusted::Input) -> Result<(), ()> {
        // NSA Guide Prerequisites:
        //
        //    Prior to accepting a verified digital signature as valid the
        //    verifier shall have:
        //
        //       1. assurance of the signatory’s claimed identity,
        //       2. an authentic copy of the domain parameters, (q, FR, a, b,
        //          SEED, G, n, h),
        //       3. assurance of the validity of the public key, and
        //       4. assurance that the claimed signatory actually possessed the
        //          private key that was used to generate the digital signature
        //          at the time that the signature was generated.
        //
        // Prerequisites #1 and #4 are outside the scope of what this function
        // can do. Prerequisite #2 is handled implicitly as the domain
        // parameters are hard-coded into the source. Prerequisite #3 is
        // handled by `parse_uncompressed_point`.
        let peer_pub_key =
            try!(parse_uncompressed_point(self.ops.public_key_ops, public_key));

        // NSA Guide Step 1: "If r and s are not both integers in the interval
        // [1, n − 1], output INVALID."
        let (r, s) = try!(signature.read_all((), |input| {
            der::nested(input, der::Tag::Sequence, (), |input| {
                let r = try!(self.ops.scalar_parse(input));
                let s = try!(self.ops.scalar_parse(input));
                Ok((r, s))
            })
        }));

        // NSA Guide Step 2: "Use the selected hash function to compute H =
        // Hash(M)."
        // NSA Guide Step 3: "Convert the bit string H to an integer e as
        // described in Appendix B.2."
        let e = digest_scalar(self.ops, self.digest_alg, msg);

        // NSA Guide Step 4: "Compute w = s**−1 mod n, using the routine in
        // Appendix B.1."
        let w = self.ops.scalar_inv_to_mont(&s);

        // NSA Guide Step 5: "Compute u1 = (e * w) mod n, and compute
        // u2 = (r * w) mod n."
        let u1 = self.ops.scalar_mul_mixed(&e, &w);
        let u2 = self.ops.scalar_mul_mixed(&r, &w);

        // NSA Guide Step 6: "Compute the elliptic curve point
        // R = (xR, yR) = u1*G + u2*Q, using EC scalar multiplication and EC
        // addition. If R is equal to the point at infinity, output INVALID."
        let product = twin_mul(self.ops.private_key_ops, &u1, &u2,
                               &peer_pub_key);

        // Verify that the point we computed is on the curve; see
        // `verify_affine_point_is_on_the_curve_scaled` for details on why. It
        // would be more secure to do the check on the affine coordinates if we
        // were going to convert to affine form (again, see
        // `verify_affine_point_is_on_the_curve_scaled` for details on why).
        // But, we're going to avoid converting to affine for performance
        // reasons, so we do the verification using the Jacobian coordinates.
        let z2 = try!(verify_jacobian_point_is_on_the_curve(
                        self.ops.public_key_ops.common, &product));

        // NSA Guide Step 7: "Compute v = xR mod n."
        // NSA Guide Step 8: "Compare v and r0. If v = r0, output VALID;
        // otherwise, output INVALID."
        //
        // Instead, we use Greg Maxwell's trick to avoid the inversion mod `q`
        // that would be necessary to compute the affine X coordinate.
        let x = self.ops.public_key_ops.common.point_x(&product);
        fn sig_r_equals_x(ops: &PublicScalarOps, r: &ElemDecoded,
                          x: &ElemUnreduced, z2: &ElemUnreduced) -> bool {
            let cops = ops.public_key_ops.common;
            let r_jacobian = cops.elem_mul_mixed(&z2, r);
            let x_decoded = cops.elem_decoded(x);
            ops.elem_decoded_equals(&r_jacobian, &x_decoded)
        }
        let r = self.ops.scalar_as_elem_decoded(&r);
        if sig_r_equals_x(self.ops, &r, &x, &z2) {
            return Ok(());
        }
        if self.ops.elem_decoded_less_than(&r, &self.ops.q_minus_n) {
            let r_plus_n =
                self.ops.elem_decoded_sum(&r,
                                          &self.ops.public_key_ops.common.n);
            if sig_r_equals_x(self.ops, &r_plus_n, &x, &z2) {
                return Ok(());
            }
        }

        Err(())
    }
}


/// Calculate the digest of `msg` using the digest algorithm `digest_alg`. Then
/// convert the digest to a scalar in the range [0, n) as described in
/// NIST's FIPS 186-4 Section 4.2. Note that this is one of the few cases where
/// a `Scalar` is allowed to have the value zero.
///
/// NIST's FIPS 186-4 4.2 says "When the length of the output of the hash
/// function is greater than N (i.e., the bit length of q), then the leftmost N
/// bits of the hash function output block shall be used in any calculation
/// using the hash function output during the generation or verification of a
/// digital signature."
///
/// "Leftmost N bits" means "N most significant bits" because we interpret the
/// digest as a bit-endian encoded integer.
///
/// The NSA guide instead vaguely suggests that we should convert the digest
/// value to an integer and then reduce it mod `n`. However, real-world
/// implementations (e.g. `digest_to_bn` in OpenSSL and `hashToInt` in Go) do
/// what FIPS 186-4 says to do, not what the NSA guide suggests.
///
/// Why shifting the value right by at most one bit is sufficient: P-256's `n`
/// has its 256th bit set; i.e. 2**255 < n < 2**256. Once we've truncated the
/// digest to 256 bits and converted it to an integer, it will have a value
/// less than 2**256. If the value is larger than `n` then shifting it one bit
/// right will give a value less than 2**255, which is less than `n`. The
/// analogous argument applies for P-384. However, it does *not* apply in
/// general; for example, it doesn't apply to P-521.
fn digest_scalar(ops: &PublicScalarOps, digest_alg: &'static digest::Algorithm,
                 msg: untrusted::Input) -> Scalar {
    let digest = digest::digest(digest_alg, msg.as_slice_less_safe());
    digest_scalar_(ops, digest.as_ref())
}

// This is a separate function solely so that we can test specific digest
// values like all-zero values and values larger than `n`.
fn digest_scalar_(ops: &PublicScalarOps, digest: &[u8]) -> Scalar {
    let num_limbs = ops.public_key_ops.common.num_limbs;

    let digest = if digest.len() > num_limbs * LIMB_BYTES {
        &digest[..(num_limbs * LIMB_BYTES)]
    } else {
        digest
    };

    // XXX: unwrap
    let mut limbs =
        parse_big_endian_value(untrusted::Input::from(digest), num_limbs)
            .unwrap();
    let n = &ops.public_key_ops.common.n.limbs[..num_limbs];
    if !limbs_less_than_limbs(&limbs[..num_limbs], n) {
        let mut carried_bit = 0;
        for i in 0..num_limbs {
            let next_carried_bit =
                limbs[num_limbs - i - 1] << (LIMB_BITS - 1);
            limbs[num_limbs - i - 1] =
                (limbs[num_limbs - i - 1] >> 1) | carried_bit;
            carried_bit = next_carried_bit;
        }
        debug_assert!(limbs_less_than_limbs(&limbs[..num_limbs], &n));
    }
    Scalar::from_limbs_unchecked(&limbs)
}

fn twin_mul(ops: &PrivateKeyOps, g_scalar: &Scalar, p_scalar: &Scalar,
            p_xy: &(Elem, Elem)) -> Point {
    // XXX: Inefficient. TODO: implement interleaved wNAF multiplication.
    let scaled_g = ops.point_mul_base(g_scalar);
    let scaled_p = ops.point_mul(p_scalar, p_xy);
    ops.common.point_sum(&scaled_g, &scaled_p)
}


macro_rules! ecdsa {
    ( $VERIFY_ALGORITHM:ident, $ecdsa_verify_ops:expr, $digest_alg:expr,
      $doc_str:expr, $use_heap_note:expr ) => {
        #[doc=$doc_str]
        ///
        /// Public keys are encoding in uncompressed form using the
        /// Octet-String-to-Elliptic-Curve-Point algorithm in [SEC 1: Elliptic
        /// Curve Cryptography, Version 2.0](http://www.secg.org/sec1-v2.pdf).
        /// Public keys are validated during key agreement as described in
        /// using the ECC
        /// Partial Public-Key Validation Routine from Section 5.6.2.3.3 of
        /// [NIST Special Publication 800-56A, revision
        /// 2](http://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-56Ar2.pdf)
        /// and Appendix A.3 of the NSA's [Suite B implementer's guide to FIPS
        /// 186-3](https://github.com/briansmith/ring/doc/ecdsa.pdf). Note
        /// that, as explained in the NSA guide, ECC Partial Public-Key
        /// Validation is equivalent to ECC Full Public-Key Validation for
        /// prime-order curves like this one.
        ///
        /// The signature will be parsed as a DER-encoded `Ecdsa-Sig-Value` as
        /// described in [RFC 3279 Section
        /// 2.2.3](https://tools.ietf.org/html/rfc3279#section-2.2.3).
        ///
        #[doc=$use_heap_note]
        pub static $VERIFY_ALGORITHM: signature::VerificationAlgorithm =
                signature::VerificationAlgorithm {
            implementation: &ECDSAVerification {
                ops: $ecdsa_verify_ops,
                digest_alg: $digest_alg,
            }
        };
    }
}

ecdsa!(ECDSA_P256_SHA1_ASN1, &p256::PUBLIC_SCALAR_OPS, &digest::SHA1,
       "Verification of ECDSA signatures using the P-256 curve and SHA-1.",
       "");
ecdsa!(ECDSA_P256_SHA256_ASN1, &p256::PUBLIC_SCALAR_OPS, &digest::SHA256,
       "Verification of ECDSA signatures using the P-256 curve and SHA-256.",
       "");
ecdsa!(ECDSA_P256_SHA384_ASN1, &p256::PUBLIC_SCALAR_OPS, &digest::SHA384,
       "Verification of ECDSA signatures using the P-256 curve and SHA-384.",
       "");
ecdsa!(ECDSA_P256_SHA512_ASN1, &p256::PUBLIC_SCALAR_OPS, &digest::SHA512,
       "Verification of ECDSA signatures using the P-256 curve and SHA-512.",
       "");

#[cfg(feature = "use_heap")]
ecdsa!(ECDSA_P384_SHA1_ASN1, &p384::PUBLIC_SCALAR_OPS, &digest::SHA1,
       "Verification of ECDSA signatures using the P-384 curve and SHA-1.",
       "Only available when the `use_heap` default feature is enabled.");
#[cfg(feature = "use_heap")]
ecdsa!(ECDSA_P384_SHA256_ASN1, &p384::PUBLIC_SCALAR_OPS, &digest::SHA256,
       "Verification of ECDSA signatures using the P-384 curve and SHA-256.",
       "Only available when the `use_heap` default feature is enabled.");
#[cfg(feature = "use_heap")]
ecdsa!(ECDSA_P384_SHA384_ASN1, &p384::PUBLIC_SCALAR_OPS, &digest::SHA384,
       "Verification of ECDSA signatures using the P-384 curve and SHA-384.",
       "Only available when the `use_heap` default feature is enabled.");
#[cfg(feature = "use_heap")]
ecdsa!(ECDSA_P384_SHA512_ASN1, &p384::PUBLIC_SCALAR_OPS, &digest::SHA512,
       "Verification of ECDSA signatures using the P-384 curve and SHA-512.",
       "Only available when the `use_heap` default feature is enabled.");


#[cfg(test)]
mod tests {
    use {digest, test, signature};
    use super::digest_scalar_;
    use super::super::ops::*;
    use untrusted;

    #[test]
    fn signature_ecdsa_verify_test() {
        test::from_file("src/ec/suite_b/ecdsa_verify_tests.txt",
                       |section, test_case| {
            assert_eq!(section, "");

            let curve_name = test_case.consume_string("Curve");
            let digest_name = test_case.consume_string("Digest");

            let msg = test_case.consume_bytes("Msg");
            let msg = untrusted::Input::from(&msg);

            let public_key = test_case.consume_bytes("Q");
            let public_key = untrusted::Input::from(&public_key);

            let sig = test_case.consume_bytes("Sig");
            let sig = untrusted::Input::from(&sig);

            let expected_result = test_case.consume_string("Result");

            let alg =
                match alg_from_curve_and_digest(&curve_name, &digest_name) {
                None => { return Ok(()); },
                Some((alg, _, _)) => alg,
            };

            let actual_result = signature::verify(alg, public_key, msg, sig);
            assert_eq!(actual_result.is_ok(), expected_result == "P (0 )");

            Ok(())
        });
    }

    #[test]
    fn ecdsa_digest_scalar_test() {
        test::from_file("src/ec/suite_b/ecdsa_digest_scalar_tests.txt",
                       |section, test_case| {
            assert_eq!(section, "");

            let curve_name = test_case.consume_string("Curve");
            let digest_name = test_case.consume_string("Digest");

            let input = test_case.consume_bytes("Input");

            let output = test_case.consume_bytes("Output");

            let (ops, digest_alg) =
                match alg_from_curve_and_digest(&curve_name, &digest_name) {
                None => { return Ok(()); },
                Some((_, ops, digest_alg)) => (ops, digest_alg)
            };

            let num_limbs = ops.public_key_ops.common.num_limbs;
            assert_eq!(input.len(), digest_alg.output_len);
            assert_eq!(output.len(),
                       ops.public_key_ops.common.num_limbs * LIMB_BYTES);

            let expected =
                try!(parse_big_endian_value(untrusted::Input::from(&output),
                                            num_limbs));

            let actual = digest_scalar_(ops, &input);

            assert_eq!(actual.limbs[..num_limbs], expected[..num_limbs]);

            Ok(())
        });
    }

    fn alg_from_curve_and_digest(
        curve_name: &str, digest_name: &str)
        -> Option<(&'static signature::VerificationAlgorithm,
                   &'static PublicScalarOps,
                   &'static digest::Algorithm)> {
        if curve_name == "P-256" {
            if digest_name == "SHA1" {
                Some((&signature::ECDSA_P256_SHA1_ASN1,
                      &p256::PUBLIC_SCALAR_OPS, &digest::SHA1))
            } else if digest_name == "SHA256" {
                Some((&signature::ECDSA_P256_SHA256_ASN1,
                      &p256::PUBLIC_SCALAR_OPS, &digest::SHA256))
            } else if digest_name == "SHA384" {
                Some((&signature::ECDSA_P256_SHA384_ASN1,
                      &p256::PUBLIC_SCALAR_OPS, &digest::SHA384))
            } else if digest_name == "SHA512" {
                Some((&signature::ECDSA_P256_SHA512_ASN1,
                      &p256::PUBLIC_SCALAR_OPS, &digest::SHA512))
            } else {
                panic!("Unsupported digest algorithm: {}", digest_name);
            }
        } else if curve_name == "P-384" {
            p384_alg_from_digest(digest_name)
        } else {
            panic!("Unsupported curve: {}", curve_name);
        }

    }

    #[cfg(feature = "use_heap")]
    fn p384_alg_from_digest(
        digest_name: &str)
        -> Option<(&'static signature::VerificationAlgorithm,
                   &'static PublicScalarOps,
                   &'static digest::Algorithm)> {
        if digest_name == "SHA1" {
            Some((&signature::ECDSA_P384_SHA1_ASN1, &p384::PUBLIC_SCALAR_OPS,
                 &digest::SHA1))
        } else if digest_name == "SHA256" {
            Some((&signature::ECDSA_P384_SHA256_ASN1, &p384::PUBLIC_SCALAR_OPS,
                 &digest::SHA256))
        } else if digest_name == "SHA384" {
            Some((&signature::ECDSA_P384_SHA384_ASN1, &p384::PUBLIC_SCALAR_OPS,
                 &digest::SHA384))
        } else if digest_name == "SHA512" {
            Some((&signature::ECDSA_P384_SHA512_ASN1, &p384::PUBLIC_SCALAR_OPS,
                 &digest::SHA512))
        } else {
            panic!("Unsupported digest algorithm: {}", digest_name);
        }
    }

    #[cfg(not(feature = "use_heap"))]
    fn p384_alg_from_digest(
        _digest_name: &str)
        -> Option<(&'static signature::VerificationAlgorithm,
                   &'static PublicScalarOps,
                   &'static digest::Algorithm)> {
        None
    }
}

#[cfg(feature = "internal_benches")]
mod benches {
    use bench;
    use {signature, test};
    use untrusted;

    #[bench]
    fn ecdsa_verify_p256_bench(bench: &mut bench::Bencher) {
        let pub_key_1 =
            test::from_hex("04e424dc61d4bb3cb7ef4344a7f8957a0c5134e16f7a67c074\
                            f82e6e12f49abf3c970eed7aa2bc48651545949de1dddaf012\
                            7e5965ac85d1243d6f60e7dfaee927").unwrap();
        let msg_1 =
            test::from_hex("e1130af6a38ccb412a9c8d13e15dbfc9e69a16385af3c3f1e5\
                            da954fd5e7c45fd75e2b8c36699228e92840c0562fbf3772f0\
                            7e17f1add56588dd45f7450e1217ad239922dd9c32695dc71f\
                            f2424ca0dec1321aa47064a044b7fe3c2b97d03ce470a59230\
                            4c5ef21eed9f93da56bb232d1eeb0035f9bf0dfafdcc460627\
                            2b20a3").unwrap();
        let sig_1 =
            test::from_hex("3045022100bf96b99aa49c705c910be33142017c642ff540c7\
                            6349b9dab72f981fd9347f4f022017c55095819089c2e03b9c\
                            d415abdf12444e323075d98f31920b9e0f57ec871c")
                           .unwrap();

        let pub_key_2 =
            test::from_hex("04e0fc6a6f50e1c57475673ee54e3a57f9a49f3328e743bf52\
                            f335e3eeaa3d28647f59d689c91e463607d9194d99faf316e2\
                            5432870816dde63f5d4b373f12f22a").unwrap();
        let msg_2 =
            test::from_hex("73c5f6a67456ae48209b5f85d1e7de7758bf235300c6ae2bdc\
                            eb1dcb27a7730fb68c950b7fcada0ecc4661d3578230f225a8\
                            75e69aaa17f1e71c6be5c831f22663bac63d0c7a9635edb004\
                            3ff8c6f26470f02a7bc56556f1437f06dfa27b487a6c4290d8\
                            bad38d4879b334e341ba092dde4e4ae694a9c09302e2dbf443\
                            581c08").unwrap();
        let sig_2 =
            test::from_hex("304502201d75830cd36f4c9aa181b2c4221e87f176b7f05b7c\
                            87824e82e396c88315c407022100cb2acb01dac96efc53a32d\
                            4a0d85d0c2e48955214783ecf50a4f0414a319c05a")
                           .unwrap();

        let vectors = [
            (untrusted::Input::from(&pub_key_1),
             untrusted::Input::from(&msg_1),
             untrusted::Input::from(&sig_1)),
            (untrusted::Input::from(&pub_key_2),
             untrusted::Input::from(&msg_2),
             untrusted::Input::from(&sig_2)),
        ];
        let mut i = 0;
        bench.iter(|| {
            let (pub_key, msg, sig) = vectors[i];
            i = (i + 1) % vectors.len();
            assert!(signature::verify(&signature::ECDSA_P256_SHA256_ASN1,
                                      pub_key, msg, sig).is_ok());
        });
    }

    #[bench]
    fn ecdsa_verify_p384_bench(bench: &mut bench::Bencher) {
        let pub_key_1 =
            test::from_hex("04cb908b1fd516a57b8ee1e14383579b33cb154fece20c5035\
                            e2b3765195d1951d75bd78fb23e00fef37d7d064fd9af144cd\
                            99c46b5857401ddcff2cf7cf822121faf1cbad9a011bed8c55\
                            1f6f59b2c360f79bfbe32adbcaa09583bdfdf7c374bb")
                            .unwrap();
        let msg_1 =
            test::from_hex("9dd789ea25c04745d57a381f22de01fb0abd3c72dbdefd44e4\
                            3213c189583eef85ba662044da3de2dd8670e6325154480155\
                            bbeebb702c75781ac32e13941860cb576fe37a05b757da5b5b\
                            418f6dd7c30b042e40f4395a342ae4dce05634c33625e2bc52\
                            4345481f7e253d9551266823771b251705b4a85166022a37ac\
                            28f1bd").unwrap();
        let sig_1 =
            test::from_hex("3064023033f64fb65cd6a8918523f23aea0bbcf56bba1daca7\
                            aff817c8791dc92428d605ac629de2e847d43cee55ba9e4a0e\
                            83ba02304428bb478a43ac73ecd6de51ddf7c28ff3c2441625\
                            a081714337dd44fea8011bae71959a10947b6ea33f77e128d3\
                            c6ae").unwrap();

        let pub_key_2 =
            test::from_hex("04a370cdbef95d1df5bf68ec487122514a107db87df3f88520\
                            68fd4694abcadb9b14302c72491a76a64442fc07bd99f02cd3\
                            97c25dc1a5781573d039f2520cf329bf65120fdbe964b6b801\
                            01160e533d5570e62125b9f3276c49244b8d0f3e44ec")
                            .unwrap();
        let msg_2 =
            test::from_hex("93e7e75cfaf3fa4e71df80f7f8c0ef6672a630d2dbeba1d613\
                            49acbaaa476f5f0e34dccbd85b9a815d908203313a22fe3e91\
                            9504cb222d623ad95662ea4a90099742c048341fe3a7a51110\
                            d30ad3a48a777c6347ea8b71749316e0dd1902facb304a7632\
                            4b71f3882e6e70319e13fc2bb9f3f5dbb9bd2cc7265f52dfc0\
                            a3bb91").unwrap();
        let sig_2 =
            test::from_hex("3065023100c6c7bb516cc3f37a304328d136b2f44bb89d3dac\
                            78f1f5bcd36b412a8b4d879f6cdb75175292c696b58bfa9c91\
                            fe639102306b711425e1b14f7224cd4b96717a84d65a60ec99\
                            51a30152ea1dd3b6ea66a0088d1fd3e9a1ef069804b7d96914\
                            8c37a0").unwrap();

        let vectors = [
            (untrusted::Input::from(&pub_key_1),
             untrusted::Input::from(&msg_1),
             untrusted::Input::from(&sig_1)),
            (untrusted::Input::from(&pub_key_2),
             untrusted::Input::from(&msg_2),
             untrusted::Input::from(&sig_2)),
        ];
        let mut i = 0;
        bench.iter(|| {
            let (pub_key, msg, sig) = vectors[i];
            i = (i + 1) % vectors.len();
            assert!(signature::verify(&signature::ECDSA_P384_SHA384_ASN1,
                                      pub_key, msg, sig).is_ok());
        });
    }
}
