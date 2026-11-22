//! CRC-32/ISCSI by fusion on SSE4.2 and PCLMULQDQ: 128-bit carry-less folds over one part of the
//! buffer while the CRC32 instruction runs over the rest.
//!
//! The method and the buffer layout are those of the kernels
//! <https://github.com/corsix/fast-crc32/> generates (MIT licensed); this is its `v4x2s4x4`
//! shape, written for this crate.

#![cfg(any(target_arch = "x86_64", target_arch = "x86"))]

#[cfg(target_arch = "x86")]
use core::arch::x86::*;

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use crate::crc32::consts::CRC32_ISCSI;
use crate::fusion::x86::*;

/// Bytes one vector step folds: four 128-bit accumulators.
const VECTOR_STEP: usize = 64;
/// CRC32 instruction streams.
const SCALAR_STREAMS: usize = 4;
/// Bytes one scalar round takes: a 64-bit word from each stream.
const SCALAR_ROUND: usize = 8 * SCALAR_STREAMS;
/// The shortest aligned length worth splitting; below it the CRC32 instruction alone is used.
const FUSION_MIN: usize = 4 * SCALAR_ROUND + 8;

/// The most scalar rounds a pass sized from the exact shift table takes.
pub(crate) const MAX_SCALAR_ROUNDS: usize = 512;
/// The most 32-byte units in one pass: its scalar rounds, and as many bytes again of vector steps.
const MAX_UNITS: usize = MAX_SCALAR_ROUNDS * 2;
/// Entries in the shift table: up to four streams' length ahead, in 64-bit words.
const SHIFTS_LEN: usize = MAX_SCALAR_ROUNDS * SCALAR_STREAMS + 1;

/// Longer passes come in these many doublings of the largest exact one.
const LONG_PASSES: usize = 40;

/// `SHIFTS[m]` is `x^(64 * m + 31) mod P`: what a partial CRC is multiplied by to move it past
/// `m` 64-bit words and into the word after them.
pub(crate) static SHIFTS: [u32; SHIFTS_LEN] = shifts();

/// The same for a pass of `MAX_SCALAR_ROUNDS << e` scalar rounds: `LONG_SHIFTS[e][j - 1]` moves
/// a partial CRC past `j` streams of that length.
pub(crate) static LONG_SHIFTS: [[u32; SCALAR_STREAMS]; LONG_PASSES] = long_shifts();

/// The reflected polynomial: a multiplication by `x` is a shift right, reduced by it.
const POLY: u32 = (CRC32_ISCSI.poly as u32).reverse_bits();

/// `a * x mod P`, in the reflected order where `x^0` is the top bit.
const fn times_x(a: u32) -> u32 {
    (a >> 1) ^ (POLY & 0u32.wrapping_sub(a & 1))
}

/// `a * b mod P`.
const fn times(mut a: u32, b: u32) -> u32 {
    let mut product = 0u32;
    let mut bit = 0;
    while bit < 32 {
        if (b >> (31 - bit)) & 1 == 1 {
            product ^= a;
        }
        a = times_x(a);
        bit += 1;
    }
    product
}

/// `x^n mod P`.
const fn x_to_the(mut n: u64) -> u32 {
    let mut result = 1u32 << 31;
    let mut square = 1u32 << 30;
    while n != 0 {
        if n & 1 == 1 {
            result = times(result, square);
        }
        square = times(square, square);
        n >>= 1;
    }
    result
}

const fn shifts() -> [u32; SHIFTS_LEN] {
    let mut table = [0u32; SHIFTS_LEN];
    let mut power = x_to_the(31);
    let mut m = 0;
    while m < SHIFTS_LEN {
        table[m] = power;
        let mut bit = 0;
        while bit < 64 {
            power = times_x(power);
            bit += 1;
        }
        m += 1;
    }
    table
}

const fn long_shifts() -> [[u32; SCALAR_STREAMS]; LONG_PASSES] {
    let mut table = [[0u32; SCALAR_STREAMS]; LONG_PASSES];
    let mut e = 0;
    while e < LONG_PASSES {
        let words = (MAX_SCALAR_ROUNDS as u64) << e;
        let mut j = 0;
        while j < SCALAR_STREAMS {
            table[e][j] = x_to_the(64 * words * (j as u64 + 1) + 31);
            j += 1;
        }
        e += 1;
    }
    table
}

/// A pair of the generated fold keys, by their indexes in the key table: `far` multiplies an
/// accumulator's low half and `near` its high half. Keys 16 and 15 (`x^(32 * 17)` and
/// `x^(32 * 15)`) carry it 64 bytes forward, 20 and 19 carry it 32 bytes, 2 and 1 carry it 16.
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn fold_keys(far: usize, near: usize) -> __m128i {
    _mm_set_epi64x(
        CRC32_ISCSI.keys.get_key(near) as i64,
        CRC32_ISCSI.keys.get_key(far) as i64,
    )
}

/// Folds `x` forward by the distance of `k` and adds the 16 bytes at `data`.
#[inline]
#[target_feature(enable = "sse2,pclmulqdq")]
unsafe fn fold_and_load(x: __m128i, k: __m128i, data: *const u8) -> __m128i {
    let lo = clmul_lo_sse(x, k);
    let hi = clmul_hi_sse(x, k);
    _mm_xor_si128(
        _mm_xor_si128(lo, _mm_loadu_si128(data as *const __m128i)),
        hi,
    )
}

/// Folds `x` forward by the distance of `k` onto `next`.
#[inline]
#[target_feature(enable = "sse2,pclmulqdq")]
unsafe fn fold_onto(x: __m128i, k: __m128i, next: __m128i) -> __m128i {
    _mm_xor_si128(_mm_xor_si128(clmul_lo_sse(x, k), next), clmul_hi_sse(x, k))
}

/// One vector step: the four accumulators fold over the next 64 bytes at `data`.
#[inline]
#[target_feature(enable = "sse2,pclmulqdq")]
unsafe fn vector_step(x: &mut [__m128i; 4], k: __m128i, data: *const u8) {
    x[0] = fold_and_load(x[0], k, data);
    x[1] = fold_and_load(x[1], k, data.add(16));
    x[2] = fold_and_load(x[2], k, data.add(32));
    x[3] = fold_and_load(x[3], k, data.add(48));
}

/// One scalar round: each stream takes its next word. Stream `n` starts `n * klen` bytes after
/// `data`.
#[inline]
#[target_feature(enable = "sse4.2")]
unsafe fn scalar_round(crc: &mut [u32; SCALAR_STREAMS], data: *const u8, klen: usize) {
    crc[0] = mm_crc32_u64(crc[0], *(data as *const u64));
    crc[1] = mm_crc32_u64(crc[1], *(data.add(klen) as *const u64));
    crc[2] = mm_crc32_u64(crc[2], *(data.add(klen * 2) as *const u64));
    crc[3] = mm_crc32_u64(crc[3], *(data.add(klen * 3) as *const u64));
}

/// CRC-32/ISCSI over `len` bytes at `buf`, continuing from `crc0`.
///
/// The aligned buffer is laid out as a vector part, four scalar streams of equal length and a
/// last 8-byte word. The vector part folds through four 128-bit accumulators while the CRC32
/// instruction runs down the four streams; the main loop pairs two vector steps (128 bytes) with
/// four scalar rounds (128 bytes), the `v4x2s4x4` shape. The five partial CRCs are then shifted
/// to the end of the streams and folded into that last word.
///
/// The two parts are sized in 32-byte units rather than whole 256-byte loop iterations, about
/// half the bytes to each, and what the main loop leaves of either is finished on its own. So
/// all but the last 31 bytes of a pass take the fused path, where a fixed block would leave up
/// to 255 of them to the CRC32 instruction alone.
///
/// The shift multipliers come from tables built at compile time, never from repeated squaring
/// at run time. On a Xeon 6731E (Sierra Forest) that squaring cost about 90 cycles for a
/// 264-byte buffer and about 270 for a 4 KiB one, as much as the checksum itself. The exact
/// table reaches passes of `MAX_UNITS` units (32 KiB); a longer buffer is taken in passes of
/// that size doubled as often as fits, largest first, so a megabyte is one pass and no length
/// needs more than a few.
///
/// # Safety
///
/// `buf` must be valid for reads of `len` bytes, and the CPU must have SSE4.2 and PCLMULQDQ.
#[inline]
#[target_feature(enable = "sse4.2,pclmulqdq")]
pub unsafe fn crc32_iscsi_sse_v4x2s4x4(mut crc0: u32, mut buf: *const u8, mut len: usize) -> u32 {
    // Align to an 8-byte boundary with the CRC32 instruction
    while len > 0 && (buf as usize & 7) != 0 {
        crc0 = _mm_crc32_u8(crc0, *buf);
        buf = buf.add(1);
        len -= 1;
    }

    while len >= FUSION_MIN + 8 {
        // On to a 16-byte boundary
        if (buf as usize & 8) != 0 {
            crc0 = mm_crc32_u64(crc0, *(buf as *const u64));
            buf = buf.add(8);
            len -= 8;
        }

        // In 32-byte units: a vector step is two, a scalar round is one. A pass takes all of a
        // buffer the exact table reaches; of a longer one it takes the largest doubling of that.
        let mut units = (len - 8) / SCALAR_ROUND;
        let long_pass = if units > MAX_UNITS {
            let mut e = (units / MAX_UNITS).ilog2() as usize;
            if e >= LONG_PASSES {
                e = LONG_PASSES - 1;
            }
            units = MAX_UNITS << e;
            Some(e)
        } else {
            None
        };
        let vector_steps = (units + 2) / 4;
        let scalar_rounds = units - vector_steps * 2;
        let vector_len = vector_steps * VECTOR_STEP;
        let klen = scalar_rounds * 8;

        // What each partial CRC is multiplied by to move it to the end of the streams
        let [shift_2, shift_1, shift_0, shift_vector] = match long_pass {
            Some(e) => LONG_SHIFTS[e],
            None => [
                SHIFTS[scalar_rounds],
                SHIFTS[scalar_rounds * 2],
                SHIFTS[scalar_rounds * 3],
                SHIFTS[scalar_rounds * 4],
            ],
        };

        let mut vector = buf;
        let mut scalar = buf.add(vector_len);

        // The first 64 bytes seed the accumulators, with the incoming CRC in the low 32 bits
        let mut x = [
            _mm_xor_si128(
                _mm_cvtsi32_si128(crc0 as i32),
                _mm_loadu_si128(vector as *const __m128i),
            ),
            _mm_loadu_si128(vector.add(16) as *const __m128i),
            _mm_loadu_si128(vector.add(32) as *const __m128i),
            _mm_loadu_si128(vector.add(48) as *const __m128i),
        ];
        vector = vector.add(VECTOR_STEP);

        // The scalar streams start from zero: the incoming CRC went into the vector part
        let mut crc = [0u32; SCALAR_STREAMS];

        let k = fold_keys(16, 15);
        let mut vector_left = vector_steps - 1;
        let mut scalar_left = scalar_rounds;

        // The main loop: two vector steps beside four scalar rounds
        while vector_left >= 2 && scalar_left >= 4 {
            vector_step(&mut x, k, vector);
            vector_step(&mut x, k, vector.add(VECTOR_STEP));
            scalar_round(&mut crc, scalar, klen);
            scalar_round(&mut crc, scalar.add(8), klen);
            scalar_round(&mut crc, scalar.add(16), klen);
            scalar_round(&mut crc, scalar.add(24), klen);

            vector = vector.add(VECTOR_STEP * 2);
            scalar = scalar.add(32);
            vector_left -= 2;
            scalar_left -= 4;
        }

        // What the main loop left of each part
        while vector_left > 0 {
            vector_step(&mut x, k, vector);
            vector = vector.add(VECTOR_STEP);
            vector_left -= 1;
        }
        while scalar_left > 0 {
            scalar_round(&mut crc, scalar, klen);
            scalar = scalar.add(8);
            scalar_left -= 1;
        }

        // Reduce the four accumulators to one: 16 bytes forward twice, then 32 bytes
        let k = fold_keys(2, 1);
        let x01 = fold_onto(x[0], k, x[1]);
        let x23 = fold_onto(x[2], k, x[3]);
        let x0 = fold_onto(x01, fold_keys(20, 19), x23);

        // And that one to 32 bits
        let vector_crc = mm_crc32_u64(
            mm_crc32_u64(0, mm_extract_epi64(x0, 0)),
            mm_extract_epi64(x0, 1),
        );

        // Shift the vector part and the first three streams to the end of the fourth, and fold
        // them into the word that follows it
        let shifted = _mm_xor_si128(
            _mm_xor_si128(
                clmul_scalar_sse(vector_crc, shift_vector),
                clmul_scalar_sse(crc[0], shift_0),
            ),
            _mm_xor_si128(
                clmul_scalar_sse(crc[1], shift_1),
                clmul_scalar_sse(crc[2], shift_2),
            ),
        );

        let fused = vector_len + klen * SCALAR_STREAMS;
        buf = buf.add(fused);
        crc0 = mm_crc32_u64(crc[3], *(buf as *const u64) ^ mm_extract_epi64(shifted, 0));
        buf = buf.add(8);
        len -= fused + 8;
    }

    // The remaining whole words
    while len >= 8 {
        crc0 = mm_crc32_u64(crc0, *(buf as *const u64));
        buf = buf.add(8);
        len -= 8;
    }

    // And the remaining bytes
    while len > 0 {
        crc0 = _mm_crc32_u8(crc0, *buf);
        buf = buf.add(1);
        len -= 1;
    }

    crc0
}
