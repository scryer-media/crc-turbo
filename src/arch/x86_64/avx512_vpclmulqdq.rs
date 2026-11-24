// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! This module provides AVX-512 and VPCLMULQDQ-specific implementations of the ArchOps trait.
//!
//! It performs folding using 4 x ZMM registers of 512-bits each.

#![cfg(target_arch = "x86_64")]

use crate::arch::x86::sse::X86SsePclmulqdqOps;
use crate::enums::Reflector;
use crate::structs::CrcState;
use crate::traits::{ArchOps, EnhancedCrcWidth};
use core::arch::x86_64::*;

/// Implements the ArchOps trait using 512-bit AVX-512 and VPCLMULQDQ instructions at 512 bits.
/// Delegates to X86SsePclmulqdqOps for standard 128-bit operations
#[derive(Debug, Copy, Clone)]
pub struct X86_64Avx512VpclmulqdqOps(X86SsePclmulqdqOps);

impl Default for X86_64Avx512VpclmulqdqOps {
    fn default() -> Self {
        Self::new()
    }
}

impl X86_64Avx512VpclmulqdqOps {
    #[inline(always)]
    pub fn new() -> Self {
        Self(X86SsePclmulqdqOps)
    }
}

// Wrapper for __m512i to make it easier to work with
#[derive(Debug, Copy, Clone)]
struct Simd512(__m512i);

impl Simd512 {
    #[inline]
    #[target_feature(enable = "avx512f")]
    #[allow(clippy::too_many_arguments)]
    unsafe fn new(x7: u64, x6: u64, x5: u64, x4: u64, x3: u64, x2: u64, x1: u64, x0: u64) -> Self {
        Self(_mm512_set_epi64(
            x7 as i64, x6 as i64, x5 as i64, x4 as i64, x3 as i64, x2 as i64, x1 as i64, x0 as i64,
        ))
    }

    #[inline]
    #[target_feature(enable = "avx512vl,vpclmulqdq")]
    unsafe fn fold_64(&self, coeff: &Self, new_data: &Self, reflected: bool) -> Self {
        let (low, high) = clmul_keys(self.0, coeff.0, reflected);
        // 512-bit ternary logic XOR3
        Self(_mm512_ternarylogic_epi64(low, high, new_data.0, 0x96))
    }

    #[cfg(feature = "std")]
    #[inline]
    #[target_feature(enable = "avx512f")]
    unsafe fn extract_u64s(&self) -> [u64; 8] {
        let mut result = [0u64; 8];
        _mm512_storeu_si512(result.as_mut_ptr().cast(), self.0);

        result
    }

    #[inline]
    #[target_feature(enable = "avx512f")]
    unsafe fn load_from_ptr(ptr: *const u8) -> Self {
        Self(_mm512_loadu_si512(ptr as *const __m512i))
    }

    #[inline]
    #[target_feature(enable = "avx512f")]
    unsafe fn xor(&self, other: &Self) -> Self {
        Self(_mm512_xor_si512(self.0, other.0))
    }

    #[cfg(feature = "std")]
    #[inline]
    #[target_feature(enable = "avx512f")]
    #[allow(unused)]
    unsafe fn print_hex(&self, prefix: &str) {
        let values = self.extract_u64s();
        println!(
            "{}={:#016x}_{:016x}_{:016x}_{:016x}_{:016x}_{:016x}_{:016x}_{:016x}",
            prefix,
            values[7],
            values[6],
            values[5],
            values[4],
            values[3],
            values[2],
            values[1],
            values[0]
        );
    }
}

impl X86_64Avx512VpclmulqdqOps {
    /// Folds `chunks` 128-byte blocks from `data`, which need not be aligned, with 4 x 512-bit
    /// registers, into one 128-bit vector ready for the final reduction.
    ///
    /// # Safety
    ///
    /// `data` must be valid for reads of `chunks * 128` bytes, and `chunks` at least 2.
    ///
    /// Inlined into the tier's entry, without a `target_feature` of its own, it was 1.39x faster
    /// at 256 bytes, 1.23x at 512 and 1.08x at 4 KiB on a Xeon Platinum 8375C, but a caller not
    /// itself inlined into the entry then calls each helper and intrinsic: forward CRCs ran 0.4x
    /// as fast from 64 KiB. So it keeps its features and stays a call.
    #[inline]
    #[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq")]
    unsafe fn fold_blocks(
        &self,
        state: &CrcState<<X86_64Avx512VpclmulqdqOps as ArchOps>::Vector>,
        data: *const u8,
        chunks: usize,
        keys: &[u64; 23],
        reflected: bool,
    ) -> __m128i {
        debug_assert!(chunks >= 2);
        // The state sits where the first bytes of the buffer do: the low 128 bits for a
        // reflected CRC, the high 128 for a forward one, whose bytes are reversed across the
        // whole register. Its other half is zero already, and the move stays in the vector
        // unit, where it used to go out through the general registers and back.
        let positioned_state = if reflected {
            Simd512(_mm512_zextsi128_si512(state.value))
        } else {
            Simd512(_mm512_inserti32x4(_mm512_setzero_si512(), state.value, 3))
        };

        let reflector = create_reflector512(reflected);

        // The first 256 bytes
        let first_ptr = data;
        let first_rest_ptr = data.add(128);

        let mut x = [
            reflect_bytes512(&reflector, Simd512::load_from_ptr(first_ptr)),
            reflect_bytes512(&reflector, Simd512::load_from_ptr(first_ptr.add(64))),
            reflect_bytes512(&reflector, Simd512::load_from_ptr(first_rest_ptr)),
            reflect_bytes512(&reflector, Simd512::load_from_ptr(first_rest_ptr.add(64))),
        ];

        x[0] = positioned_state.xor(&x[0]);

        let coeff = self.create_avx512_256byte_coefficient(keys);

        let remaining = chunks - 2;
        let pair_count = remaining / 2;

        for i in 0..pair_count {
            let block1_ptr = data.add(256 + i * 256);
            let block2_ptr = block1_ptr.add(128);

            x[0] = x[0].fold_64(
                &coeff,
                &reflect_bytes512(&reflector, Simd512::load_from_ptr(block1_ptr)),
                reflected,
            );
            x[1] = x[1].fold_64(
                &coeff,
                &reflect_bytes512(&reflector, Simd512::load_from_ptr(block1_ptr.add(64))),
                reflected,
            );
            x[2] = x[2].fold_64(
                &coeff,
                &reflect_bytes512(&reflector, Simd512::load_from_ptr(block2_ptr)),
                reflected,
            );
            x[3] = x[3].fold_64(
                &coeff,
                &reflect_bytes512(&reflector, Simd512::load_from_ptr(block2_ptr.add(64))),
                reflected,
            );
        }

        if remaining % 2 == 1 {
            // We have 1 unprocessed block (128 bytes)
            // Fold 4×512 down to 2×512 and process the remaining block with 2-register mode
            let folded_2reg = self.fold_from_4x512_to_2x256(x, keys, reflected);
            let coeff_2reg = self.create_avx512_128byte_coefficient(keys);

            let last_block_ptr = data.add((chunks - 1) * 128);

            let final_x = [
                folded_2reg[0].fold_64(
                    &coeff_2reg,
                    &reflect_bytes512(&reflector, Simd512::load_from_ptr(last_block_ptr)),
                    reflected,
                ),
                folded_2reg[1].fold_64(
                    &coeff_2reg,
                    &reflect_bytes512(&reflector, Simd512::load_from_ptr(last_block_ptr.add(64))),
                    reflected,
                ),
            ];

            return self.fold_from_2x512_to_1x128(final_x, keys, reflected);
        }

        // All blocks processed in pairs - fold from 4 x 512-bit to 1 x 128-bit
        self.fold_from_4x512_to_1x128(x, keys, reflected)
    }

    /// [`ArchOps::fold_short_wide`] for a buffer's first eight to fifteen blocks: the first eight
    /// as two 512-bit registers, each lane folded straight onto the eighth block in one step,
    /// and the blocks after those in 256-bit registers with that result as their head. In
    /// 256-bit registers alone the eight blocks were three multiplies deep. From twelve blocks
    /// the first four are folded 128 bytes on, onto blocks eight to eleven, before that step,
    /// and the 256-bit tail starts at the twelfth: at 192 bytes, against folding the four
    /// blocks after the eighth in 256-bit registers, CRC-32/ISO-HDLC was 1.40x faster on a
    /// Xeon Platinum 8375C, 1.26x on an EPYC 9R14 and 1.06x on a Xeon Platinum 8488C, and the
    /// other algorithms 1.12x to 1.29x.
    ///
    /// # Safety
    ///
    /// `data` must be valid for reads of `units * 16` bytes, and `units` from 8 to 15.
    #[inline]
    #[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq")]
    unsafe fn fold_short_512(
        &self,
        head: __m128i,
        data: *const u8,
        units: usize,
        keys: &[u64; 23],
        reflected: bool,
    ) -> __m128i {
        debug_assert!((8..16).contains(&units));
        let positioned_state = if reflected {
            Simd512(_mm512_zextsi128_si512(head))
        } else {
            Simd512(_mm512_inserti32x4(_mm512_setzero_si512(), head, 3))
        };
        let reflector = create_reflector512(reflected);
        let first =
            reflect_bytes512(&reflector, Simd512::load_from_ptr(data)).xor(&positioned_state);
        let second = reflect_bytes512(&reflector, Simd512::load_from_ptr(data.add(64)));
        let (x, done) = if units >= 12 {
            let third = reflect_bytes512(&reflector, Simd512::load_from_ptr(data.add(128)));
            let coeff = self.create_avx512_128byte_coefficient(keys);
            ([second, first.fold_64(&coeff, &third, reflected)], 12)
        } else {
            ([first, second], 8)
        };
        let folded = self.fold_from_2x512_to_1x128(x, keys, reflected);
        if units == done {
            return folded;
        }
        crate::arch::x86_64::avx2_vpclmulqdq::fold_short_256(
            folded,
            data.add(done * 16),
            units - done + 1,
            false,
            keys,
            reflected,
        )
    }

    /// Create a folding coefficient for AVX-512 for 128-byte folding distances
    #[inline(always)]
    unsafe fn create_avx512_128byte_coefficient(&self, keys: &[u64; 23]) -> Simd512 {
        key_pair512(keys, 3)
    }

    /// Create a folding coefficient for AVX-512 for 256-byte folding distances
    #[inline(always)]
    unsafe fn create_avx512_256byte_coefficient(&self, keys: &[u64; 23]) -> Simd512 {
        key_pair512(keys, 21)
    }

    /// Fold from 4 x 512-bit to 1 x 128-bit
    #[inline(always)]
    unsafe fn fold_from_4x512_to_1x128(
        &self,
        x: [Simd512; 4],
        keys: &[u64; 23],
        reflected: bool,
    ) -> __m128i {
        // Step 1: Fold 4 x 512-bit to 2 x 512-bit
        let x2 = self.fold_from_4x512_to_2x256(x, keys, reflected);

        // Step 2: Fold 2 x 512-bit to 1 x 128-bit
        self.fold_from_2x512_to_1x128(x2, keys, reflected)
    }

    /// Fold from 4 x 512-bit to 2 x 512-bit
    #[inline(always)]
    unsafe fn fold_from_4x512_to_2x256(
        &self,
        x: [Simd512; 4],
        keys: &[u64; 23],
        reflected: bool,
    ) -> [Simd512; 2] {
        // This folds registers that are 128 bytes apart (x[0] with x[2], x[1] with x[3])
        let coeff = self.create_avx512_128byte_coefficient(keys);

        // Fold pairs:
        // x[0] (bytes 0-63) + x[2] (bytes 128-191) → result[0]
        // x[1] (bytes 64-127) + x[3] (bytes 192-255) → result[1]
        [
            x[0].fold_64(&coeff, &x[2], reflected),
            x[1].fold_64(&coeff, &x[3], reflected),
        ]
    }

    /// Fold from 2 x 512-bit to 1 x 128-bit in one step: each of the eight lanes onto the last
    /// lane by its own distance, 112 bytes down to 16, in two multiplies that do not wait on
    /// each other, with a coefficient per lane, and the eight lanes xored together. Folding
    /// the first register onto the second and then that one's lanes was one multiply deeper,
    /// on the path every buffer of 256 bytes or more ends with.
    #[inline(always)]
    unsafe fn fold_from_2x512_to_1x128(
        &self,
        x: [Simd512; 2],
        keys: &[u64; 23],
        reflected: bool,
    ) -> __m128i {
        // Reflected streams hold the earliest 16 bytes in lane 0; forward ones, byte-reversed
        // per 64 bytes, hold them in lane 3. Each lane takes the key for its distance from the
        // last 16 bytes, and the last lane, which does not move, a zero key.
        //
        // The keys for 112, 96, 80 and 64 bytes are keys[9..17], and those for 48 and 32 are
        // keys[17..21], in that order, so each coefficient is a load, with the 16-byte pair
        // from keys[1..3] set beside the second: built a key at a time they went through the
        // general registers and the port the multiplies run on. A forward CRC has its lanes
        // the other way round.
        let key = keys.as_ptr();
        let first_keys = _mm512_loadu_si512(key.add(9).cast());
        let second_keys = _mm512_inserti32x4(
            _mm512_zextsi256_si512(_mm256_loadu_si256(key.add(17).cast())),
            _mm_loadu_si128(key.add(1).cast()),
            2,
        );
        let (first_keys, second_keys, last_lane) = if reflected {
            (first_keys, second_keys, 0b1100_0000)
        } else {
            (
                _mm512_shuffle_i64x2(first_keys, first_keys, 0b00_01_10_11),
                _mm512_shuffle_i64x2(second_keys, second_keys, 0b00_01_10_11),
                0b0000_0011,
            )
        };

        let (low, high) = clmul_keys(x[0].0, first_keys, reflected);
        let first =
            _mm512_ternarylogic_epi64(low, high, _mm512_maskz_mov_epi64(last_lane, x[1].0), 0x96);
        let (low, high) = clmul_keys(x[1].0, second_keys, reflected);
        let lanes = _mm512_ternarylogic_epi64(low, high, first, 0x96);
        let halves = _mm256_xor_si256(
            _mm512_castsi512_si256(lanes),
            _mm512_extracti64x4_epi64(lanes, 1),
        );

        _mm_xor_si128(
            _mm256_castsi256_si128(halves),
            _mm256_extracti128_si256(halves, 1),
        )
    }
}

/// The fold coefficient `keys[low]` and `keys[low + 1]`, in each of the four lanes, in the
/// order they are stored: one broadcast load. [`clmul_keys`] multiplies by it.
#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn key_pair512(keys: &[u64; 23], low: usize) -> Simd512 {
    let pair: &[u64; 2] = keys[low..low + 2].try_into().unwrap_unchecked();
    Simd512(_mm512_broadcast_i32x4(_mm_loadu_si128(
        pair.as_ptr().cast(),
    )))
}

/// The two products of a fold: each lane of `x` by its lane of `keys`, a pair of generated keys
/// in the order they are stored. A reflected CRC multiplies its low half by the second key and
/// its high half by the first, a forward CRC the other way round; the multiply picks the
/// halves, so the keys are never swapped, a shuffle that ran on the multiplies' own port.
#[inline]
#[target_feature(enable = "avx512f,vpclmulqdq")]
unsafe fn clmul_keys(x: __m512i, keys: __m512i, reflected: bool) -> (__m512i, __m512i) {
    if reflected {
        (
            _mm512_clmulepi64_epi128(x, keys, 0x10),
            _mm512_clmulepi64_epi128(x, keys, 0x01),
        )
    } else {
        (
            _mm512_clmulepi64_epi128(x, keys, 0x00),
            _mm512_clmulepi64_epi128(x, keys, 0x11),
        )
    }
}

// 512-bit version of the Reflector
#[derive(Clone, Copy)]
enum Reflector512 {
    NoReflector,
    ForwardReflector { smask: Simd512 },
}

// Function to create the appropriate reflector based on CRC parameters
#[inline(always)]
unsafe fn create_reflector512(reflected: bool) -> Reflector512 {
    if reflected {
        Reflector512::NoReflector
    } else {
        // Load shuffle mask
        let smask = Simd512::new(
            0x08090a0b0c0d0e0f,
            0x0001020304050607,
            0x08090a0b0c0d0e0f,
            0x0001020304050607,
            0x08090a0b0c0d0e0f,
            0x0001020304050607,
            0x08090a0b0c0d0e0f,
            0x0001020304050607,
        );
        Reflector512::ForwardReflector { smask }
    }
}

// Function to apply reflection to a 512-bit vector
#[inline(always)]
unsafe fn reflect_bytes512(reflector: &Reflector512, data: Simd512) -> Simd512 {
    match reflector {
        Reflector512::NoReflector => data,
        Reflector512::ForwardReflector { smask } => shuffle_bytes512(data, *smask),
    }
}

// pre-compute the reverse indices for 512-bit shuffling
static REVERSE_INDICES_512: __m512i =
    unsafe { core::mem::transmute([7u64, 6u64, 5u64, 4u64, 3u64, 2u64, 1u64, 0u64]) };

// Implement a 512-bit byte shuffle function
#[inline]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn shuffle_bytes512(data: Simd512, mask: Simd512) -> Simd512 {
    Simd512(_mm512_permutexvar_epi64(
        // Reverse the order using 512-bit permutation
        REVERSE_INDICES_512,                 // reverse indices
        _mm512_shuffle_epi8(data.0, mask.0), // shuffled data
    ))
}

// Delegate all ArchOps methods to the inner X86SsePclmulqdqOps instance
impl ArchOps for X86_64Avx512VpclmulqdqOps {
    type Vector = __m128i;

    #[inline(always)]
    unsafe fn process_enhanced_simd_blocks<W: EnhancedCrcWidth>(
        &self,
        state: &mut CrcState<Self::Vector>,
        first: &[Self::Vector; 8],
        rest: &[[Self::Vector; 8]],
        _reflector: &Reflector<Self::Vector>,
        keys: &[u64; 23],
    ) -> bool
    where
        Self::Vector: Copy,
    {
        let reflected = state.reflected;
        let folded = self.fold_blocks(
            state,
            first.as_ptr() as *const u8,
            1 + rest.len(),
            keys,
            reflected,
        );
        *state = W::create_state(
            W::perform_final_reduction(folded, reflected, keys, self),
            reflected,
            self,
        );

        // Return true to indicate we handled it
        true
    }

    #[inline(always)]
    unsafe fn fold_short_wide(
        &self,
        head: Self::Vector,
        data: *const u8,
        units: usize,
        head_in_data: bool,
        keys: &[u64; 23],
        reflected: bool,
    ) -> Option<Self::Vector> {
        if head_in_data && units >= 8 {
            return Some(self.fold_short_512(head, data, units, keys, reflected));
        }
        Some(crate::arch::x86_64::avx2_vpclmulqdq::fold_short_256(
            head,
            data,
            units,
            head_in_data,
            keys,
            reflected,
        ))
    }

    #[inline(always)]
    unsafe fn fold_simd_blocks_unaligned(
        &self,
        state: &CrcState<Self::Vector>,
        data: *const u8,
        chunks: usize,
        keys: &[u64; 23],
    ) -> Option<Self::Vector> {
        // The 512-bit fold starts from two chunks, 256 bytes.
        if chunks < 2 {
            return None;
        }
        Some(self.fold_blocks(state, data, chunks, keys, state.reflected))
    }

    // Delegate all other methods to X86SsePclmulqdqOps
    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn create_vector_from_u64_pair(
        &self,
        high: u64,
        low: u64,
        reflected: bool,
    ) -> Self::Vector {
        self.0.create_vector_from_u64_pair(high, low, reflected)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn create_vector_from_u64_pair_non_reflected(
        &self,
        high: u64,
        low: u64,
    ) -> Self::Vector {
        self.0.create_vector_from_u64_pair_non_reflected(high, low)
    }

    #[inline]
    #[target_feature(enable = "sse4.1")]
    unsafe fn create_vector_from_u64(&self, value: u64, high: bool) -> Self::Vector {
        self.0.create_vector_from_u64(value, high)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn extract_u64s(&self, vector: Self::Vector) -> [u64; 2] {
        self.0.extract_u64s(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn extract_poly64s(&self, vector: Self::Vector) -> [u64; 2] {
        self.0.extract_poly64s(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn xor_vectors(&self, a: Self::Vector, b: Self::Vector) -> Self::Vector {
        self.0.xor_vectors(a, b)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn load_bytes(&self, ptr: *const u8) -> Self::Vector {
        self.0.load_bytes(ptr)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn load_aligned(&self, ptr: *const [u64; 2]) -> Self::Vector {
        self.0.load_aligned(ptr)
    }

    #[inline]
    #[target_feature(enable = "ssse3")]
    unsafe fn shuffle_bytes(&self, data: Self::Vector, mask: Self::Vector) -> Self::Vector {
        self.0.shuffle_bytes(data, mask)
    }

    #[inline]
    #[target_feature(enable = "sse4.1")]
    unsafe fn blend_vectors(
        &self,
        a: Self::Vector,
        b: Self::Vector,
        mask: Self::Vector,
    ) -> Self::Vector {
        self.0.blend_vectors(a, b, mask)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_left_8(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_left_8(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn set_all_bytes(&self, value: u8) -> Self::Vector {
        self.0.set_all_bytes(value)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn create_compare_mask(&self, vector: Self::Vector) -> Self::Vector {
        self.0.create_compare_mask(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn and_vectors(&self, a: Self::Vector, b: Self::Vector) -> Self::Vector {
        self.0.and_vectors(a, b)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_right_32(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_right_32(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_left_32(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_left_32(vector)
    }

    #[inline]
    #[target_feature(enable = "sse4.1")]
    unsafe fn create_vector_from_u32(&self, value: u32, high: bool) -> Self::Vector {
        self.0.create_vector_from_u32(value, high)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_left_4(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_left_4(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_right_4(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_right_4(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_right_8(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_right_8(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_right_5(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_right_5(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_right_6(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_right_6(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_right_7(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_right_7(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_right_12(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_right_12(vector)
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn shift_left_12(&self, vector: Self::Vector) -> Self::Vector {
        self.0.shift_left_12(vector)
    }

    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    unsafe fn carryless_mul_00(&self, a: Self::Vector, b: Self::Vector) -> Self::Vector {
        self.0.carryless_mul_00(a, b)
    }

    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    unsafe fn carryless_mul_01(&self, a: Self::Vector, b: Self::Vector) -> Self::Vector {
        self.0.carryless_mul_01(a, b)
    }

    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    unsafe fn carryless_mul_10(&self, a: Self::Vector, b: Self::Vector) -> Self::Vector {
        self.0.carryless_mul_10(a, b)
    }

    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    unsafe fn carryless_mul_11(&self, a: Self::Vector, b: Self::Vector) -> Self::Vector {
        self.0.carryless_mul_11(a, b)
    }

    #[inline]
    #[target_feature(enable = "avx512vl")]
    unsafe fn xor3_vectors(
        &self,
        a: Self::Vector,
        b: Self::Vector,
        c: Self::Vector,
    ) -> Self::Vector {
        // VPCLMULQDQ implementation always uses AVX512 ternary logic
        // Feature detection is handled at the dispatch level
        _mm_ternarylogic_epi64(
            a, b, c, 0x96, // XOR3 operation
        )
    }
}
