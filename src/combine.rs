//! Combining the CRCs of adjacent byte ranges, and the other operations that rest on the same
//! arithmetic: undoing a combine, and the CRC of a run of zero bytes.
//!
//! Appending `n` bytes to a CRC's input multiplies the register by `x^(8n)` modulo the
//! polynomial `P(x)` before the new bytes' own contribution is added, so
//!
//! ```text
//! crc(A || B) = crc(A) * x^(8 * len(B)) + crc(B)
//! ```
//!
//! once the initial value and the final XOR are accounted for. Everything here is arithmetic in
//! `GF(2)[x] / P(x)`: `x^(8n) mod P(x)` by square-and-multiply over the bits of `n`, and one
//! field multiply to apply it. A field multiply is a carry-less multiply and a Barrett reduction
//! where the CPU has a carry-less multiply instruction (PCLMULQDQ on x86_64, PMULL on aarch64),
//! and a shift-and-add loop otherwise. The field arithmetic follows crc-fast#60 by dcbtsla.
//!
//! crc-fast combined with Mark Adler's GF(2) zeros operator (`crccomb.c`): a `width x width` bit
//! matrix squared once per bit of `n`. That construction is kept below, in the tests, as the
//! independent reference this module is checked against; its notice follows.

/*
  Derived from this excellent answer by Mark Adler on StackOverflow:
  https://stackoverflow.com/questions/29915764/generic-crc-8-16-32-64-combine-implementation/29928573#29928573
*/

/* crccomb.c -- generalized combination of CRCs
 * Copyright (C) 2015 Mark Adler
 * Version 1.1  29 Apr 2015  Mark Adler
 */

/*
 This software is provided 'as-is', without any express or implied
 warranty.  In no event will the author be held liable for any damages
 arising from the use of this software.

 Permission is granted to anyone to use this software for any purpose,
 including commercial applications, and to alter it and redistribute it
 freely, subject to the following restrictions:

 1. The origin of this software must not be misrepresented; you must not
    claim that you wrote the original software. If you use this software
    in a product, an acknowledgment in the product documentation would be
    appreciated but is not required.
 2. Altered source versions must be plainly marked as such, and must not be
    misrepresented as being the original software.
 3. This notice may not be removed or altered from any source distribution.

 Mark Adler
 madler@alumni.caltech.edu
*/

use crate::arch::clmul::Clmul;
use crate::{CrcError, CrcParams};

/// The low `width` bits.
#[inline(always)]
const fn mask(width: u8) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

/// Reverses the low `width` bits, converting between the reflected and the normal form.
#[inline(always)]
const fn reflect(value: u64, width: u8) -> u64 {
    value.reverse_bits() >> (64 - width as u32)
}

/// `a * b mod P(x)` for a `width`-bit polynomial `P(x) = x^width + poly` in normal form, by
/// shift and add: `b` is consumed a bit at a time while `a` is doubled, reduced whenever doubling
/// overflows the field.
///
/// Branch-free, because both conditions are data the predictor cannot learn.
const fn mul_mod_portable(mut a: u64, mut b: u64, poly: u64, width: u8) -> u64 {
    let msb_shift = width as u32 - 1;
    let mask = mask(width);

    let mut product = 0u64;
    let mut n = 0;
    while n < width {
        product ^= a & 0u64.wrapping_sub(b & 1);
        b >>= 1;
        let overflow = 0u64.wrapping_sub((a >> msb_shift) & 1);
        a = ((a << 1) & mask) ^ (poly & overflow);
        n += 1;
    }

    product
}

/// `floor(x^(2 * width) / P(x))` without its leading `x^width` term, which does not fit a `u64`
/// at width 64 and is applied as a separate XOR instead.
///
/// Long division of `x^(2 * width)` by `P(x)`, one dividend bit at a time from the top.
const fn barrett_mu(poly: u64, width: u8) -> u64 {
    let mask = mask(width);
    let top = width as u32 - 1;

    let mut remainder = 0u64;
    let mut quotient = 0u64;
    let mut pos = 2 * width as u32 + 1;
    while pos > 0 {
        pos -= 1;
        let bit = (pos == 2 * width as u32) as u64;
        // A bit shifted out past `width - 1` means `P(x)` divides here: the quotient takes a
        // one and the remainder is reduced by `poly`.
        let carry = (remainder >> top) & 1;
        remainder = (((remainder << 1) | bit) & mask) ^ (poly & 0u64.wrapping_sub(carry));
        quotient = (quotient << 1) | carry;
    }

    quotient & mask
}

/// `GF(2)[x] / P(x)` for one CRC polynomial, in normal form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Field {
    /// `P(x)` without its leading `x^width` term.
    poly: u64,
    width: u8,
    /// [`barrett_mu`] of `poly`.
    mu: u64,
    /// The carry-less multiply [`Field::mul`] may use, when the selected tier has one.
    clmul: Option<Clmul>,
}

impl Field {
    #[inline]
    fn new(poly: u64, width: u8) -> Self {
        assert!(
            matches!(width, 16 | 32 | 64),
            "Unsupported CRC width: {width}"
        );

        Self {
            poly: poly & mask(width),
            width,
            mu: barrett_mu(poly & mask(width), width),
            clmul: Clmul::detect(),
        }
    }

    /// `a * b mod P(x)`.
    #[inline]
    fn mul(&self, a: u64, b: u64) -> u64 {
        if let Some(clmul) = self.clmul {
            return clmul.mul_mod(a, b, self.poly, self.mu, self.width);
        }

        mul_mod_portable(a, b, self.poly, self.width)
    }

    /// `base^(8 * n) mod P(x)`, by square-and-multiply over the bits of `n`.
    #[inline]
    fn pow_bytes(&self, base: u64, mut n: u64) -> u64 {
        // base^8: one byte
        let mut square = self.mul(base, base);
        square = self.mul(square, square);
        square = self.mul(square, square);

        let mut power = 1u64;
        while n != 0 {
            if n & 1 == 1 {
                power = self.mul(power, square);
            }
            n >>= 1;
            if n != 0 {
                square = self.mul(square, square);
            }
        }

        power
    }

    /// `x^-1 mod P(x)`, which exists when `P(x)` has an `x^0` term, as every CRC polynomial
    /// does: `x * (x^(width - 1) + poly / x) = x^width + poly + 1`, which is `1` mod `P(x)`.
    #[inline]
    fn x_inverse(&self) -> Option<u64> {
        (self.poly & 1 == 1).then(|| (1u64 << (self.width - 1)) | (self.poly >> 1))
    }
}

/// What a built-in algorithm's combines share: the field and its `x^(2^k) mod P(x)` table, so
/// that `x^(8n)` costs one multiply per set bit of `n` rather than a square per bit as well.
/// This is zlib's `x2n_table`, built at first use instead of stored.
#[cfg(any(
    feature = "std",
    all(
        feature = "alloc",
        any(target_arch = "aarch64", target_arch = "x86", target_arch = "x86_64")
    )
))]
mod powers {
    use super::{mask, Field};
    use crate::{CrcAlgorithm, CrcParams};
    #[cfg(not(feature = "std"))]
    use alloc::boxed::Box;

    /// Entries in a table: `x^(2^k)` for `k < 67` covers `x^(8n)` for every `u64` `n`.
    const X2N_LEN: usize = 67;

    pub(super) struct Powers {
        pub(super) field: Field,
        pub(super) x2n: [u64; X2N_LEN],
    }

    impl Powers {
        fn new(params: &CrcParams) -> Self {
            let field = Field::new(params.poly, params.width);
            let mut x2n = [0u64; X2N_LEN];
            x2n[0] = 1 << 1; // x
            for k in 1..X2N_LEN {
                x2n[k] = field.mul(x2n[k - 1], x2n[k - 1]);
            }

            Self { field, x2n }
        }

        /// `x^(8 * n) mod P(x)`: one multiply per set bit of `n`.
        #[inline]
        pub(super) fn pow_bytes(&self, n: u64) -> u64 {
            let mut power = 1u64;
            let mut bits = n;
            while bits != 0 {
                // Bit j of n is x^(8 * 2^j) = x^(2^(j + 3)).
                power = self
                    .field
                    .mul(power, self.x2n[bits.trailing_zeros() as usize + 3]);
                bits &= bits - 1;
            }

            power
        }
    }

    /// One slot per `CrcAlgorithm` discriminant. Boxed, so an unused slot costs a pointer.
    const SLOTS: usize = 64;

    #[cfg(feature = "std")]
    type Slot = std::sync::OnceLock<Box<Powers>>;
    #[cfg(not(feature = "std"))]
    type Slot = spin::Once<Box<Powers>>;

    #[allow(clippy::declare_interior_mutable_const)]
    const EMPTY: Slot = Slot::new();
    static TABLES: [Slot; SLOTS] = [EMPTY; SLOTS];

    /// The table for `params`' algorithm, when it is a built-in one and `params` still describes
    /// it. Parameters built by hand under a built-in name fall back to plain square-and-multiply.
    #[allow(deprecated)]
    pub(super) fn get(params: &CrcParams) -> Option<&'static Powers> {
        if matches!(
            params.algorithm,
            CrcAlgorithm::CrcCustom | CrcAlgorithm::Crc32Custom | CrcAlgorithm::Crc64Custom
        ) {
            return None;
        }

        let slot = TABLES.get(params.algorithm as usize)?;
        let init = || {
            Box::new(Powers::new(
                &crate::get_calculator_params(params.algorithm).1,
            ))
        };
        #[cfg(feature = "std")]
        let powers = slot.get_or_init(init);
        #[cfg(not(feature = "std"))]
        let powers = slot.call_once(init);

        (powers.field.width == params.width
            && powers.field.poly == params.poly & mask(params.width))
        .then_some(&**powers)
    }
}

/// The effect on a CRC of appending a fixed number of bytes to its input: what combining the
/// CRCs of two adjacent ranges, undoing such a combine, and the CRC of a run of zero bytes all
/// need, computed once for that length.
///
/// Building one costs a square-and-multiply over the bits of the length, a few dozen field
/// multiplies; each use after that is a single field multiply. A caller combining many ranges of
/// the same length (fixed-size blocks, segments, slices) should build one and reuse it.
///
/// # Examples
///
/// ```rust
/// use crc_turbo::{checksum, CrcAlgorithm::Crc32IsoHdlc, CrcCombine};
///
/// let first = checksum(Crc32IsoHdlc, b"1234");
/// let second = checksum(Crc32IsoHdlc, b"56789");
///
/// let five_bytes = CrcCombine::new(Crc32IsoHdlc, 5);
/// let combined = five_bytes.combine(first, second);
/// assert_eq!(combined, 0xcbf43926);
///
/// // Knowing the suffix, the prefix's CRC comes back out.
/// assert_eq!(five_bytes.uncombine(combined, second), Some(first));
///
/// // The CRC of five zero bytes, without five zero bytes.
/// assert_eq!(five_bytes.zeros(), checksum(Crc32IsoHdlc, &[0; 5]));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrcCombine {
    field: Field,
    reflected: bool,
    /// The algorithm's initial value, in the form the CRC state takes.
    init_algorithm: u64,
    xorout: u64,
    len: u64,
    /// `x^(8 * len) mod P(x)`.
    power: u64,
}

impl CrcCombine {
    /// The operator for appending `len` bytes, for a CRC with these parameters.
    ///
    /// # Panics
    ///
    /// If the parameters mix reflected input with unreflected output or the reverse, which no
    /// catalogued CRC of a supported width does, or if the width is not 16, 32 or 64;
    /// [`CrcCombine::try_new_with_params`] returns the error instead.
    #[inline]
    pub fn new_with_params(params: CrcParams, len: u64) -> Self {
        Self::from_params(&params, len)
    }

    /// [`CrcCombine::new_with_params`] that returns [`CrcError::MismatchedReflection`] or
    /// [`CrcError::UnsupportedWidth`] instead of panicking.
    #[inline]
    pub fn try_new_with_params(params: CrcParams, len: u64) -> Result<Self, CrcError> {
        Self::try_from_params(&params, len)
    }

    /// [`CrcCombine::new_with_params`] without moving the parameters, which carry the fold keys.
    #[inline]
    pub(crate) fn from_params(params: &CrcParams, len: u64) -> Self {
        match Self::try_from_params(params, len) {
            Ok(combine) => combine,
            Err(error) => panic!("{error}"),
        }
    }

    /// [`CrcCombine::try_new_with_params`] without moving the parameters.
    #[inline]
    pub(crate) fn try_from_params(params: &CrcParams, len: u64) -> Result<Self, CrcError> {
        if !matches!(params.width, 16 | 32 | 64) {
            return Err(CrcError::UnsupportedWidth(params.width));
        }
        if params.refin != params.refout {
            return Err(CrcError::MismatchedReflection);
        }

        #[cfg(any(
            feature = "std",
            all(
                feature = "alloc",
                any(target_arch = "aarch64", target_arch = "x86", target_arch = "x86_64")
            )
        ))]
        let (field, power) = match powers::get(params) {
            Some(powers) => (powers.field, powers.pow_bytes(len)),
            None => {
                let field = Field::new(params.poly, params.width);
                (field, field.pow_bytes(1 << 1, len))
            }
        };
        #[cfg(not(any(
            feature = "std",
            all(
                feature = "alloc",
                any(target_arch = "aarch64", target_arch = "x86", target_arch = "x86_64")
            )
        )))]
        let (field, power) = {
            let field = Field::new(params.poly, params.width);
            (field, field.pow_bytes(1 << 1, len))
        };

        Ok(Self {
            field,
            reflected: params.refin,
            init_algorithm: params.init_algorithm & mask(params.width),
            xorout: params.xorout & mask(params.width),
            len,
            power,
        })
    }

    /// The number of bytes this operator appends.
    #[inline]
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether this operator appends nothing, and so leaves a CRC unchanged.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Converts a CRC value into the field's normal form.
    #[inline(always)]
    fn to_field(self, value: u64) -> u64 {
        if self.reflected {
            reflect(value, self.field.width)
        } else {
            value
        }
    }

    /// The CRC of a range followed by this operator's `len` bytes, from `checksum1`, the CRC of
    /// the range, and `checksum2`, the CRC of the `len` bytes.
    #[inline]
    pub fn combine(&self, checksum1: u64, checksum2: u64) -> u64 {
        let width = self.field.width;
        let shifted = self.field.mul(
            self.to_field((checksum1 ^ self.init_algorithm ^ self.xorout) & mask(width)),
            self.power,
        );

        self.to_field(shifted) ^ (checksum2 & mask(width))
    }

    /// The CRC of a range from `combined`, the CRC of that range followed by this operator's
    /// `len` bytes, and `checksum2`, the CRC of those `len` bytes: [`CrcCombine::combine`]
    /// undone.
    ///
    /// Costs more than building the operator: the inverse of its power is a full
    /// square-and-multiply on every call.
    /// `None` only for a custom polynomial without an `x^0` term, whose CRC loses information
    /// and so cannot be undone.
    pub fn uncombine(&self, combined: u64, checksum2: u64) -> Option<u64> {
        let width = self.field.width;
        let inverse = self.field.pow_bytes(self.field.x_inverse()?, self.len);
        let unshifted = self
            .field
            .mul(self.to_field((combined ^ checksum2) & mask(width)), inverse);

        Some(self.to_field(unshifted) ^ self.init_algorithm ^ self.xorout)
    }

    /// The CRC of `len` zero bytes.
    #[inline]
    pub fn zeros(&self) -> u64 {
        // The register starts at the initial value and each zero byte multiplies it by x^8.
        self.to_field(
            self.field
                .mul(self.to_field(self.init_algorithm), self.power),
        ) ^ self.xorout
    }
}

/* Combine the CRCs of two successive sequences, where crc1 is the CRC of the
first sequence of bytes, crc2 is the CRC of the immediately following
sequence of bytes, and len2 is the length of the second sequence.  The CRC
of the combined sequence is returned. */
#[inline]
pub fn checksums(crc1: u64, crc2: u64, len2: u64, params: &CrcParams) -> u64 {
    CrcCombine::from_params(params, len2).combine(crc1, crc2)
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::test::consts::TEST_ALL_CONFIGS;
    use crate::test::enums::AnyCrcTestConfig;

    /* Multiply the GF(2) vector vec by the GF(2) matrix mat, returning the
    resulting vector.  The vector is stored as bits in a crc_t.  The matrix is
    similarly stored with each column as a crc_t, where the number of columns is
    at least enough to cover the position of the most significant 1 bit in the
    vector (so a dimension parameter is not needed). */
    fn gf2_matrix_times(mat: &[u64; 64], mut vec: u64) -> u64 {
        let mut sum = 0;
        let mut idx = 0;
        while vec > 0 {
            if vec & 1 == 1 {
                sum ^= mat[idx];
            }
            vec >>= 1;
            idx += 1;
        }

        sum
    }

    /* Multiply the matrix mat by itself, returning the result in square.  WIDTH is
    the dimension of the matrices, i.e., the number of bits in each crc_t
    (rows), and the number of crc_t's (columns). */
    fn gf2_matrix_square(square: &mut [u64; 64], mat: &[u64; 64]) {
        for n in 0..64 {
            square[n] = gf2_matrix_times(mat, mat[n]);
        }
    }

    /// crc-fast 1.10's combine: Mark Adler's zeros operator, unchanged but for its name and the
    /// `reflect` it calls. The
    /// independent reference the field arithmetic is checked against.
    fn combine_by_matrix(mut crc1: u64, crc2: u64, mut len2: u64, params: &CrcParams) -> u64 {
        let mut col: u64;
        let mut even = [0u64; 64]; /* even-power-of-two zeros operator */
        let mut odd = [0u64; 64]; /* odd-power-of-two zeros operator */

        /* exclusive-or the result with len2 zeros applied to the CRC of an empty
        sequence */
        crc1 ^= params.init_algorithm ^ params.xorout;

        /* construct the operator for one zero bit and put in odd[] */
        if params.refin && params.refout {
            // use the reflected POLY
            odd[0] = reflect(params.poly, params.width);
            col = 1;
            for n in 1..params.width {
                odd[n as usize] = col;
                col <<= 1;
            }
        } else if !params.refin && !params.refout {
            col = 2;
            for n in 0..params.width - 1 {
                odd[n as usize] = col;
                col <<= 1;
            }
            // Put poly at the last valid index (width-1)
            odd[(params.width - 1) as usize] = params.poly;
        } else {
            panic!("Unsupported CRC configuration");
        }

        /* put operator for two zero bits in even */
        gf2_matrix_square(&mut even, &odd);

        /* put operator for four zero bits in odd */
        gf2_matrix_square(&mut odd, &even);

        /* apply len2 zeros to crc1 (first square will put the operator for one
        zero byte, eight zero bits, in even) */
        loop {
            /* apply zeros operator for this bit of len2 */
            gf2_matrix_square(&mut even, &odd);
            if len2 & 1 == 1 {
                crc1 = gf2_matrix_times(&even, crc1);
            }
            len2 >>= 1;

            /* if no more bits set, then done */
            if len2 == 0 {
                break;
            }

            /* another iteration of the loop with odd and even swapped */
            gf2_matrix_square(&mut odd, &even);
            if len2 & 1 == 1 {
                crc1 = gf2_matrix_times(&odd, crc1);
            }
            len2 >>= 1;

            /* if no more bits set, then done */
            if len2 == 0 {
                break;
            }
        }

        /* return combined crc */
        crc1 ^= crc2;

        crc1
    }

    /// Deterministic xorshift64* stream.
    struct Stream(u64);

    impl Stream {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        fn bytes(&mut self, len: usize) -> Vec<u8> {
            (0..len).map(|_| self.next() as u8).collect()
        }
    }

    /// The software (table-driven) CRC: the oracle.
    fn software(config: &AnyCrcTestConfig, data: &[u8]) -> u64 {
        let params = config.get_params();
        crate::arch::software::update(params.init_algorithm, data, params) ^ params.xorout
    }

    fn lengths(stream: &mut Stream) -> Vec<u64> {
        let mut lengths = vec![
            0,
            1,
            2,
            3,
            7,
            8,
            15,
            16,
            17,
            255,
            256,
            4095,
            4096,
            1 << 20,
            (1 << 32) - 1,
            1 << 32,
            (1 << 32) + 1,
            u64::MAX >> 1,
            u64::MAX,
        ];
        lengths.extend((0..24).map(|_| stream.next() % (1 << 24)));
        lengths.extend((0..8).map(|_| stream.next()));
        lengths
    }

    #[test]
    fn portable_and_clmul_multiplies_agree_with_the_matrix_reference() {
        let mut stream = Stream(0x9e37_79b9_7f4a_7c15);

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let crc1 = stream.next() & mask(params.width);
            let crc2 = stream.next() & mask(params.width);

            for len2 in lengths(&mut stream) {
                let expected = combine_by_matrix(crc1, crc2, len2, params);

                let mut op = CrcCombine::new_with_params(*params, len2);
                assert_eq!(
                    op.combine(crc1, crc2),
                    expected,
                    "{} len {len2}",
                    config.get_name()
                );

                // the same operator built and applied in software
                op.field.clmul = None;
                op.power = op.field.pow_bytes(1 << 1, len2);
                assert_eq!(
                    op.combine(crc1, crc2),
                    expected,
                    "{} len {len2} (portable)",
                    config.get_name()
                );
            }
        }
    }

    #[test]
    fn every_built_in_algorithm_takes_the_table() {
        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let powers = powers::get(params)
                .unwrap_or_else(|| panic!("{} has no power table", config.get_name()));
            assert_eq!(powers.field, Field::new(params.poly, params.width));
        }
    }

    #[test]
    fn parameters_relabelled_with_a_built_in_name_skip_its_table() {
        let mut stream = Stream(0x2545_f491_4f6c_dd1d);
        let mut params = *TEST_ALL_CONFIGS[0].get_params();
        params.poly ^= 0x8; // a different field under the same name
        assert!(powers::get(&params).is_none());

        for len2 in lengths(&mut stream) {
            let crc1 = stream.next() & mask(params.width);
            let crc2 = stream.next() & mask(params.width);
            assert_eq!(
                CrcCombine::new_with_params(params, len2).combine(crc1, crc2),
                combine_by_matrix(crc1, crc2, len2, &params),
                "len {len2}"
            );
        }
    }

    #[test]
    fn combine_matches_the_crc_of_the_concatenation() {
        let mut stream = Stream(0x5eed_cafe_dead_beef);

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            for _ in 0..16 {
                let a_len = (stream.next() % 600) as usize;
                let a = stream.bytes(a_len);
                let b_len = (stream.next() % 600) as usize;
                let b = stream.bytes(b_len);
                let ab: Vec<u8> = a.iter().chain(&b).copied().collect();

                let op = CrcCombine::new_with_params(*params, b.len() as u64);
                let (crc_a, crc_b) = (software(config, &a), software(config, &b));
                let crc_ab = software(config, &ab);

                assert_eq!(op.combine(crc_a, crc_b), crc_ab, "{}", config.get_name());
                assert_eq!(
                    checksums(crc_a, crc_b, b.len() as u64, params),
                    crc_ab,
                    "{}",
                    config.get_name()
                );
                assert_eq!(
                    op.uncombine(crc_ab, crc_b),
                    Some(crc_a),
                    "{}",
                    config.get_name()
                );
            }
        }
    }

    #[test]
    fn uncombine_undoes_combine_at_every_length() {
        let mut stream = Stream(0x0123_4567_89ab_cdef);

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            for len2 in lengths(&mut stream) {
                let crc1 = stream.next() & mask(params.width);
                let crc2 = stream.next() & mask(params.width);
                let op = CrcCombine::new_with_params(*params, len2);

                assert_eq!(
                    op.uncombine(op.combine(crc1, crc2), crc2),
                    Some(crc1),
                    "{} len {len2}",
                    config.get_name()
                );
            }
        }
    }

    #[test]
    fn zeros_match_the_crc_of_zero_bytes() {
        let zeros = [0u8; 1100];

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            for len in (0..=40).chain([255, 256, 257, 1023, 1024, 1100]) {
                assert_eq!(
                    CrcCombine::new_with_params(*params, len as u64).zeros(),
                    software(config, &zeros[..len]),
                    "{} len {len}",
                    config.get_name()
                );
            }
        }
    }

    #[test]
    fn zeros_of_huge_lengths_combine_like_their_halves() {
        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            for half in [1u64 << 31, (1 << 40) + 3, u64::MAX / 2] {
                let half_zeros = CrcCombine::new_with_params(*params, half);
                let whole = CrcCombine::new_with_params(*params, 2 * half);

                assert_eq!(
                    half_zeros.combine(half_zeros.zeros(), half_zeros.zeros()),
                    whole.zeros(),
                    "{} len {half}",
                    config.get_name()
                );
            }
        }
    }

    #[test]
    fn empty_operator_is_the_identity_on_well_formed_input() {
        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let empty = CrcCombine::new_with_params(*params, 0);
            let crc_empty = software(config, &[]);
            let crc = software(config, b"123456789");

            assert!(empty.is_empty());
            assert_eq!(empty.combine(crc, crc_empty), crc, "{}", config.get_name());
            assert_eq!(empty.zeros(), crc_empty, "{}", config.get_name());
        }
    }

    #[test]
    fn clmul_multiply_matches_the_portable_multiply() {
        let mut stream = Stream(0x0f1e_2d3c_4b5a_6978);

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let clmul = Field::new(params.poly, params.width);
            let portable = Field {
                clmul: None,
                ..clmul
            };
            if clmul.clmul.is_none() {
                eprintln!("skipping: no carry-less multiply on this host");
                return;
            }

            for _ in 0..256 {
                let (a, b) = (
                    stream.next() & mask(params.width),
                    stream.next() & mask(params.width),
                );
                assert_eq!(
                    clmul.mul(a, b),
                    portable.mul(a, b),
                    "{} {a:#x} * {b:#x}",
                    config.get_name()
                );
            }
            // the extremes of the field
            let top = mask(params.width);
            assert_eq!(
                clmul.mul(top, top),
                portable.mul(top, top),
                "{}",
                config.get_name()
            );
        }
    }
}
