// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! This module provides an AVX2 and VPCLMULQDQ implementation of the ArchOps trait, for CPUs
//! that have 256-bit VPCLMULQDQ but no AVX-512 (Intel Alder Lake through Arrow Lake, and other
//! AVX2-only parts with VPCLMULQDQ).
//!
//! It performs folding using 4 x YMM registers of 256 bits each: eight 128-bit streams, the same
//! stream layout as the SSE tier's fold-by-8 loop, folded two streams per carry-less multiply.
//! Every CRC width and bit order uses it through the same keys the SSE and AVX-512 tiers use.

#![cfg(target_arch = "x86_64")]

use crate::arch::x86::sse::X86SsePclmulqdqOps;
use crate::enums::Reflector;
use crate::structs::CrcState;
use crate::traits::{ArchOps, EnhancedCrcWidth};
use core::arch::x86_64::*;

/// Implements the ArchOps trait using 256-bit AVX2 and VPCLMULQDQ instructions.
/// Delegates to X86SsePclmulqdqOps for standard 128-bit operations
///
/// `PREFETCH` has the fold loop ask for the data `PREFETCH_AHEAD` bytes on, for the CPUs
/// whose own prefetchers do not keep up with it: see [`Self::prefetching`].
#[derive(Debug, Copy, Clone)]
pub struct X86_64Avx2VpclmulqdqOps<const PREFETCH: bool = false>(X86SsePclmulqdqOps);

/// How far ahead of the fold the prefetching loop asks for its data: on a Xeon 6731E 2 KiB
/// to 8 KiB were level up to 32 MiB, and at 64 MiB 4 KiB was about a fifth ahead of 2 KiB.
const PREFETCH_AHEAD: usize = 4096;

impl Default for X86_64Avx2VpclmulqdqOps {
    fn default() -> Self {
        Self::new()
    }
}

impl X86_64Avx2VpclmulqdqOps {
    #[inline(always)]
    pub fn new() -> Self {
        Self(X86SsePclmulqdqOps)
    }
}

impl X86_64Avx2VpclmulqdqOps<true> {
    /// The same fold with a software prefetch in its loop, for Intel's E-core parts, whose
    /// hardware prefetchers do not keep up with it past the L2: on a Xeon 6731E it cost
    /// nothing from 64 KiB to 1 MiB and was 6% ahead at 8 MiB, 10% at 16 MiB, about 20% at
    /// 32 MiB and 1.4 to 2 times at 64 MiB, over all 50 algorithms.
    #[inline(always)]
    pub fn prefetching() -> Self {
        Self(X86SsePclmulqdqOps)
    }
}

// Wrapper for __m256i to make it easier to work with
#[derive(Debug, Copy, Clone)]
struct Simd256(__m256i);

impl Simd256 {
    #[inline]
    #[target_feature(enable = "avx")]
    unsafe fn new(x3: u64, x2: u64, x1: u64, x0: u64) -> Self {
        Self(_mm256_set_epi64x(
            x3 as i64, x2 as i64, x1 as i64, x0 as i64,
        ))
    }

    #[inline]
    #[target_feature(enable = "avx2,vpclmulqdq")]
    unsafe fn fold_32(&self, coeff: &Self, new_data: &Self) -> Self {
        Self(_mm256_xor_si256(
            _mm256_xor_si256(
                _mm256_clmulepi64_epi128(self.0, coeff.0, 0), // Low parts
                _mm256_clmulepi64_epi128(self.0, coeff.0, 17), // High parts
            ),
            new_data.0,
        ))
    }

    #[inline]
    #[target_feature(enable = "avx")]
    unsafe fn load_from_ptr(ptr: *const u8) -> Self {
        Self(_mm256_loadu_si256(ptr as *const __m256i))
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    unsafe fn xor(&self, other: &Self) -> Self {
        Self(_mm256_xor_si256(self.0, other.0))
    }
}

impl<const PREFETCH: bool> X86_64Avx2VpclmulqdqOps<PREFETCH> {
    /// Folds `chunks` 128-byte blocks from `data`, which need not be aligned, with 4 x 256-bit
    /// registers, into one 128-bit vector ready for the final reduction.
    ///
    /// # Safety
    ///
    /// `data` must be valid for reads of `chunks * 128` bytes, and `chunks` at least 1.
    ///
    /// Unlike the AVX-512 tier's, it keeps its own `target_feature` and stays a call: inlined
    /// into the tier's entry it was 1% to 3% slower from 384 to 512 bytes on an EPYC 7R13.
    #[inline]
    #[target_feature(enable = "avx2,vpclmulqdq")]
    unsafe fn fold_blocks(
        &self,
        state: &CrcState<<X86_64Avx2VpclmulqdqOps as ArchOps>::Vector>,
        data: *const u8,
        chunks: usize,
        keys: &[u64; 23],
        reflected: bool,
    ) -> __m128i {
        debug_assert!(chunks >= 1);
        let state_u64s = self.extract_u64s(state.value);

        // Forward CRCs hold each 128-bit stream byte-reversed, as the SSE tier does, so the
        // state goes in the high half of the first stream.
        let positioned_state = if reflected {
            Simd256::new(0, 0, 0, state_u64s[0])
        } else {
            Simd256::new(0, 0, state_u64s[1], 0)
        };

        let reflector = create_reflector256(reflected);

        // The first 128 bytes, as 8 streams in 4 registers
        let first_ptr = data;
        let mut x = [
            reflect_bytes256(&reflector, Simd256::load_from_ptr(first_ptr)),
            reflect_bytes256(&reflector, Simd256::load_from_ptr(first_ptr.add(32))),
            reflect_bytes256(&reflector, Simd256::load_from_ptr(first_ptr.add(64))),
            reflect_bytes256(&reflector, Simd256::load_from_ptr(first_ptr.add(96))),
        ];

        x[0] = positioned_state.xor(&x[0]);

        // Each register moves 128 bytes forward per block: the SSE fold-by-8 distance
        let coeff = self.create_avx2_128byte_coefficient(keys, reflected);

        for chunk in 1..chunks {
            let block_ptr = data.add(chunk * 128);

            if PREFETCH {
                // Two lines per 128-byte block; a prefetch past the end does not fault
                let ahead = block_ptr.wrapping_add(PREFETCH_AHEAD) as *const i8;
                _mm_prefetch::<_MM_HINT_T0>(ahead);
                _mm_prefetch::<_MM_HINT_T0>(ahead.wrapping_add(64));
            }

            x[0] = x[0].fold_32(
                &coeff,
                &reflect_bytes256(&reflector, Simd256::load_from_ptr(block_ptr)),
            );
            x[1] = x[1].fold_32(
                &coeff,
                &reflect_bytes256(&reflector, Simd256::load_from_ptr(block_ptr.add(32))),
            );
            x[2] = x[2].fold_32(
                &coeff,
                &reflect_bytes256(&reflector, Simd256::load_from_ptr(block_ptr.add(64))),
            );
            x[3] = x[3].fold_32(
                &coeff,
                &reflect_bytes256(&reflector, Simd256::load_from_ptr(block_ptr.add(96))),
            );
        }

        self.fold_from_4x256_to_1x128(x, keys, reflected)
    }

    /// Create a folding coefficient for AVX2 for 128-byte folding distances
    #[inline(always)]
    unsafe fn create_avx2_128byte_coefficient(&self, keys: &[u64; 23], reflected: bool) -> Simd256 {
        let (k1, k2) = if reflected {
            (keys[3], keys[4])
        } else {
            (keys[4], keys[3])
        };

        // Replicate the coefficient pair
        Simd256::new(k1, k2, k1, k2)
    }

    /// Fold from 4 x 256-bit to 1 x 128-bit in two levels: each register onto the one 64
    /// bytes on, then each lane of the two left onto the last lane by its own distance, with a
    /// coefficient per lane, and the lanes xored together. Eight 256-bit multiplies in place of
    /// fourteen 128-bit ones.
    #[inline(always)]
    unsafe fn fold_from_4x256_to_1x128(
        &self,
        x: [Simd256; 4],
        keys: &[u64; 23],
        reflected: bool,
    ) -> __m128i {
        // Both bit orders hold the earlier 16 bytes of a register in lane 0. The lanes of the
        // first register left are 48 and 32 bytes from the last 16; of the second, 16 and 0,
        // and the last lane, which does not move, takes a zero key.
        let pair = |high: u64, low: u64| if reflected { (high, low) } else { (low, high) };
        let (h64, l64) = pair(keys[15], keys[16]);
        let (h48, l48) = pair(keys[17], keys[18]);
        let (h32, l32) = pair(keys[19], keys[20]);
        let (h16, l16) = pair(keys[1], keys[2]);
        let coeff_64 = Simd256::new(h64, l64, h64, l64);
        let first_coeffs = Simd256::new(h32, l32, h48, l48);
        let second_coeffs = Simd256::new(0, 0, h16, l16);

        let a = x[0].fold_32(&coeff_64, &x[2]);
        let b = x[1].fold_32(&coeff_64, &x[3]);

        let lanes = _mm256_xor_si256(
            _mm256_xor_si256(
                _mm256_xor_si256(
                    _mm256_clmulepi64_epi128(a.0, first_coeffs.0, 0x00),
                    _mm256_clmulepi64_epi128(a.0, first_coeffs.0, 0x11),
                ),
                _mm256_xor_si256(
                    _mm256_clmulepi64_epi128(b.0, second_coeffs.0, 0x00),
                    _mm256_clmulepi64_epi128(b.0, second_coeffs.0, 0x11),
                ),
            ),
            _mm256_blend_epi32(_mm256_setzero_si256(), b.0, 0b1111_0000),
        );

        _mm_xor_si128(
            _mm256_castsi256_si128(lanes),
            _mm256_extracti128_si256(lanes, 1),
        )
    }
}

/// [`ArchOps::fold_short_wide`] in 256-bit registers, two blocks per carry-less multiply.
/// The blocks are taken in 256-bit pairs and halved, in the manner of libdeflate's and the
/// Linux kernel's x86 CRC templates: up to four pairs folded two onto two across 64 bytes, then
/// a pair across 64 bytes onto each, one pair onto the other across 32 bytes, then a pair
/// across 32 bytes, the pair's halves across 16 bytes, and a last block across 16 bytes. With
/// 128-bit registers the same blocks take twice the multiplies, which on an EPYC 7R13 bound
/// a buffer of 128 to 255 bytes. The AVX-512 tier takes it too.
///
/// # Safety
///
/// As for [`ArchOps::fold_short_wide`], and the CPU must have AVX2 and VPCLMULQDQ.
#[inline]
#[target_feature(enable = "avx2,vpclmulqdq")]
pub(crate) unsafe fn fold_short_256(
    head: __m128i,
    data: *const u8,
    units: usize,
    head_in_data: bool,
    keys: &[u64; 23],
    reflected: bool,
) -> __m128i {
    debug_assert!((2..16).contains(&units));
    // Block `i` is at `base + 16 * i`; without `head_in_data`, block 0 is `head`, and `base`
    // itself is never read.
    let base = if head_in_data {
        data
    } else {
        data.wrapping_sub(16)
    };
    let reflector = create_reflector256(reflected);
    macro_rules! pair_at {
        ($unit:expr) => {
            reflect_bytes256(&reflector, Simd256::load_from_ptr(base.add($unit * 16)))
        };
    }
    macro_rules! block_at {
        ($unit:expr) => {{
            let block = _mm_loadu_si128(base.add($unit * 16) as *const __m128i);
            match reflector {
                Reflector256::NoReflector => block,
                Reflector256::ForwardReflector { smask } => {
                    _mm_shuffle_epi8(block, _mm256_castsi256_si128(smask.0))
                }
            }
        }};
    }
    // The coefficient pairs, high and low per 128-bit lane, for each distance, as
    // `fold_from_4x256_to_1x128` makes them.
    macro_rules! coeff {
        ($high:expr, $low:expr) => {{
            let (h, l) = if reflected {
                (keys[$high], keys[$low])
            } else {
                (keys[$low], keys[$high])
            };
            Simd256::new(h, l, h, l)
        }};
    }

    // From the buffer's start, one load and the state into its low lane, as libdeflate does,
    // where inserting a lane is a step more on the chain to the result.
    let mut v0 = if head_in_data {
        pair_at!(0).xor(&Simd256(_mm256_zextsi128_si256(head)))
    } else {
        Simd256(_mm256_inserti128_si256(
            _mm256_castsi128_si256(head),
            block_at!(1),
            1,
        ))
    };
    if units >= 4 {
        let k32 = coeff!(19, 20);
        let mut v1 = pair_at!(2);
        let mut next = 4;
        if units >= 8 {
            let k64 = coeff!(15, 16);
            v0 = v0.fold_32(&k64, &pair_at!(4));
            v1 = v1.fold_32(&k64, &pair_at!(6));
            next = 8;
            if units & 4 != 0 {
                v0 = v0.fold_32(&k64, &pair_at!(8));
                v1 = v1.fold_32(&k64, &pair_at!(10));
                next = 12;
            }
        }
        v0 = v0.fold_32(&k32, &v1);
        if units & 2 != 0 {
            v0 = v0.fold_32(&k32, &pair_at!(next));
        }
    }

    let k16 = _mm256_castsi256_si128(coeff!(1, 2).0);
    // A macro rather than a closure, which LLVM may leave out of line without the features.
    macro_rules! fold_16 {
        ($x:expr, $next:expr) => {{
            let x = $x;
            _mm_xor_si128(
                _mm_xor_si128(
                    _mm_clmulepi64_si128(x, k16, 0x00),
                    _mm_clmulepi64_si128(x, k16, 0x11),
                ),
                $next,
            )
        }};
    }
    let mut x = fold_16!(
        _mm256_castsi256_si128(v0.0),
        _mm256_extracti128_si256(v0.0, 1)
    );
    if units & 1 != 0 {
        x = fold_16!(x, block_at!(units - 1));
    }
    x
}

// 256-bit version of the Reflector
#[derive(Clone, Copy)]
enum Reflector256 {
    NoReflector,
    ForwardReflector { smask: Simd256 },
}

// Function to create the appropriate reflector based on CRC parameters
#[inline(always)]
unsafe fn create_reflector256(reflected: bool) -> Reflector256 {
    if reflected {
        Reflector256::NoReflector
    } else {
        // Byte-reverse each 128-bit lane, which is the SSE tier's per-block reflection; an
        // in-lane shuffle avoids a lane-crossing permute on every load
        let smask = Simd256::new(
            0x0001020304050607,
            0x08090a0b0c0d0e0f,
            0x0001020304050607,
            0x08090a0b0c0d0e0f,
        );
        Reflector256::ForwardReflector { smask }
    }
}

// Function to apply reflection to a 256-bit vector
#[inline(always)]
unsafe fn reflect_bytes256(reflector: &Reflector256, data: Simd256) -> Simd256 {
    match reflector {
        Reflector256::NoReflector => data,
        Reflector256::ForwardReflector { smask } => shuffle_bytes256(data, *smask),
    }
}

// Reverse the 16 bytes of each 128-bit lane
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn shuffle_bytes256(data: Simd256, mask: Simd256) -> Simd256 {
    Simd256(_mm256_shuffle_epi8(data.0, mask.0))
}

// Delegate all ArchOps methods to the inner X86SsePclmulqdqOps instance
impl<const PREFETCH: bool> ArchOps for X86_64Avx2VpclmulqdqOps<PREFETCH> {
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
        Some(fold_short_256(
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
    #[target_feature(enable = "sse4.1")]
    unsafe fn xor3_vectors(
        &self,
        a: Self::Vector,
        b: Self::Vector,
        c: Self::Vector,
    ) -> Self::Vector {
        self.0.xor3_vectors(a, b, c)
    }
}
