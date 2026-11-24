//! CRC-32/ISCSI for long buffers on the AVX-512 VPCLMULQDQ tier: four 512-bit fold lanes with
//! five CRC32 instruction streams beside them.
//!
//! The shape is fast-crc32's `v4s5x3` (<https://github.com/corsix/fast-crc32/>, MIT), written
//! here from its description rather than converted from its output: a core with one port for
//! 512-bit carry-less multiplies and another for the CRC32 instruction runs both at once, so the
//! streams' bytes are nearly free. The fold keys are worked out at compile time by
//! [`iscsi_fold_keys`].

#![cfg(target_arch = "x86_64")]

use crate::crc32::fusion::powers::iscsi_fold_keys;
use crate::fusion::x86::*;

/// Bytes the fold lanes take per block.
const VECTOR: usize = 256;
/// CRC32 instruction streams.
const STREAMS: usize = 5;
/// Bytes each stream takes per block: three words.
const STREAM: usize = 24;
/// Bytes per block.
const BLOCK: usize = VECTOR + STREAMS * STREAM;

const K_256: [u64; 2] = iscsi_fold_keys(256);
const K_192: [u64; 2] = iscsi_fold_keys(192);
const K_128: [u64; 2] = iscsi_fold_keys(128);
const K_64: [u64; 2] = iscsi_fold_keys(64);
/// A key per 128-bit lane, for its distance from the last one, which does not move.
const K_LANES: [u64; 8] = {
    let (k48, k32, k16) = (
        iscsi_fold_keys(48),
        iscsi_fold_keys(32),
        iscsi_fold_keys(16),
    );
    [k48[0], k48[1], k32[0], k32[1], k16[0], k16[1], 0, 0]
};

/// `a * b * x^33 mod P`, for two multipliers in the form [`xnmodp_iscsi_sse`] returns: the
/// CRC32 instruction multiplies the 64-bit product by `x^32`, and the product of two 32-bit
/// reflected polynomials sits one bit low in its 64.
#[inline]
#[target_feature(enable = "sse4.2,pclmulqdq")]
unsafe fn times(a: u32, b: u32) -> u32 {
    _mm_crc32_u64(0, _mm_cvtsi128_si64(clmul_scalar_sse(a, b)) as u64) as u32
}

/// The blocks of `len` bytes this kernel folds: a multiple of eight, so that the streams' bytes
/// ahead of the fold lanes are a multiple of 64 and the lanes stay on the cache lines the head
/// was aligned to.
#[inline(always)]
const fn blocks(len: usize) -> usize {
    (len / BLOCK) & !7
}

/// CRC-32/ISCSI over `len` bytes at `buf`.
///
/// The buffer is taken in blocks of 376 bytes: each stream runs over 24 bytes per block, one
/// stream's bytes after another's, and the lanes fold the 256 bytes per block that follow all
/// five. Each stream's CRC is then multiplied past the bytes after it and added to the fold's.
/// What is left after the last block goes to the three-lane kernel.
///
/// # Safety
///
/// `buf` must be readable for `len` bytes, and the CPU must have AVX512VL, AVX512BW,
/// VPCLMULQDQ, PCLMULQDQ and SSE4.2.
// Out of line for the reason the three-lane kernel is.
#[inline(never)]
#[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq,pclmulqdq,sse4.2")]
pub unsafe fn crc32_iscsi_avx512_vpclmulqdq_v4s5x3(
    mut crc0: u32,
    mut buf: *const u8,
    mut len: usize,
) -> u32 {
    let head = buf.align_offset(64).min(len);
    crc0 = crc32_iscsi_small_unaligned(crc0, core::slice::from_raw_parts(buf, head));
    buf = buf.add(head);
    len -= head;

    let blk = blocks(len);
    if blk != 0 {
        let klen = blk * STREAM;
        let mut lanes = buf.add(STREAMS * klen);
        let load = |p: *const u8| _mm512_loadu_si512(p as *const __m512i);
        let key = |k: &[u64; 2]| _mm512_broadcast_i32x4(_mm_loadu_si128(k.as_ptr().cast()));
        macro_rules! fold {
            ($x:expr, $k:expr, $data:expr) => {
                _mm512_ternarylogic_epi64(
                    _mm512_clmulepi64_epi128($x, $k, 0x00),
                    _mm512_clmulepi64_epi128($x, $k, 0x11),
                    $data,
                    0x96,
                )
            };
        }

        let mut x = [
            load(lanes),
            load(lanes.add(64)),
            load(lanes.add(128)),
            load(lanes.add(192)),
        ];
        lanes = lanes.add(VECTOR);
        let mut crc = [crc0 as u64, 0, 0, 0, 0];
        macro_rules! streams {
            () => {{
                let mut word = 0;
                while word < STREAM {
                    let mut stream = 0;
                    while stream < STREAMS {
                        let at = buf.add(stream * klen + word).cast::<u64>();
                        crc[stream] = _mm_crc32_u64(crc[stream], at.read_unaligned());
                        stream += 1;
                    }
                    word += 8;
                }
            }};
        }

        let k = key(&K_256);
        for _ in 1..blk {
            x[0] = fold!(x[0], k, load(lanes));
            x[1] = fold!(x[1], k, load(lanes.add(64)));
            x[2] = fold!(x[2], k, load(lanes.add(128)));
            x[3] = fold!(x[3], k, load(lanes.add(192)));
            streams!();
            buf = buf.add(STREAM);
            lanes = lanes.add(VECTOR);
        }
        streams!();

        // The four registers onto the last, each by its own distance, then that one's four
        // lanes onto its last the same way.
        let (k192, k128, k64) = (key(&K_192), key(&K_128), key(&K_64));
        let folded = _mm512_xor_si512(
            fold!(x[0], k192, x[3]),
            _mm512_ternarylogic_epi64(
                _mm512_clmulepi64_epi128(x[1], k128, 0x00),
                _mm512_clmulepi64_epi128(x[1], k128, 0x11),
                _mm512_xor_si512(
                    _mm512_clmulepi64_epi128(x[2], k64, 0x00),
                    _mm512_clmulepi64_epi128(x[2], k64, 0x11),
                ),
                0x96,
            ),
        );
        let k_lanes = _mm512_loadu_si512(K_LANES.as_ptr().cast());
        let lanes_folded = fold!(folded, k_lanes, _mm512_maskz_mov_epi64(0b1100_0000, folded));
        let halves = _mm256_xor_si256(
            _mm512_castsi512_si256(lanes_folded),
            _mm512_extracti64x4_epi64(lanes_folded, 1),
        );
        let last = _mm_xor_si128(
            _mm256_castsi256_si128(halves),
            _mm256_extracti128_si256(halves, 1),
        );

        // Each stream's CRC past the streams after it and the lanes' bytes: the last stream's
        // multiplier, then one more stream's length for each stream before it, `times` cancelling
        // the step's `x^-33`.
        let step = xnmodp_iscsi_sse(klen as u64 * 8 - 33);
        let mut shift = xnmodp_iscsi_sse((blk * VECTOR) as u64 * 8 - 33);
        let mut shifted = 0u64;
        let mut stream = STREAMS;
        while stream > 0 {
            stream -= 1;
            shifted ^= _mm_cvtsi128_si64(clmul_scalar_sse(crc[stream] as u32, shift)) as u64;
            shift = times(shift, step);
        }

        // 128 bits to 32, which multiplies by x^32.
        let low = _mm_crc32_u64(0, _mm_cvtsi128_si64(last) as u64);
        crc0 = _mm_crc32_u64(low, shifted ^ _mm_extract_epi64(last, 1) as u64) as u32;

        buf = lanes;
        len -= blk * BLOCK;
    }

    super::avx512_vpclmulqdq::crc32_iscsi_avx512_vpclmulqdq_v3x2(crc0, buf, len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc32::consts::CRC32_ISCSI;

    /// The kernel against the software path, the oracle: around its first block count and the
    /// next, from every alignment of the head within a cache line, from a carried state.
    #[test]
    fn matches_software() {
        if !(is_x86_feature_detected!("avx512vl")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("vpclmulqdq")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.2"))
        {
            return;
        }

        let data: Vec<u8> = (0..40_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let first = 8 * BLOCK;
        let mut lengths = vec![0, 1, 63, 64, 383, 384, 4 * BLOCK, 39_000];
        for near in [first, 2 * first, 3 * first, 12 * first] {
            lengths.extend([
                near - 65,
                near - 1,
                near,
                near + 1,
                near + 63,
                near + 64,
                near + 400,
            ]);
        }
        for offset in [0usize, 1, 3, 8, 31, 63] {
            for &len in &lengths {
                let bytes = &data[offset..offset + len];
                for state in [0xffff_ffffu32, 0, 0x1234_5678] {
                    let expected =
                        crate::arch::software::update(state as u64, bytes, &CRC32_ISCSI) as u32;
                    // SAFETY: the features were detected above
                    let got = unsafe {
                        crc32_iscsi_avx512_vpclmulqdq_v4s5x3(state, bytes.as_ptr(), bytes.len())
                    };
                    assert_eq!(
                        got, expected,
                        "offset {offset}, length {len}, state {state:#x}"
                    );
                }
            }
        }
    }

    /// The run-time product is the compile-time one, times `x^33`.
    #[test]
    fn times_is_the_product() {
        if !(is_x86_feature_detected!("pclmulqdq") && is_x86_feature_detected!("sse4.2")) {
            return;
        }
        let poly = (CRC32_ISCSI.poly as u32).reverse_bits();
        for (a, b) in [
            (1000u64, 31u64),
            (64, 64),
            (8 * 24 * 8, 8 * 2048 * 8 - 33),
            (5, 1 << 20),
        ] {
            let (pa, pb) = (
                crate::crc32::fusion::powers::x_to_the(a, poly),
                crate::crc32::fusion::powers::x_to_the(b, poly),
            );
            // SAFETY: the features were detected above
            let product = unsafe { times(pa, pb) };
            assert_eq!(
                product,
                crate::crc32::fusion::powers::x_to_the(a + b + 33, poly),
                "{a} + {b}"
            );
            // SAFETY: as above
            assert_eq!(unsafe { xnmodp_iscsi_sse(a) }, pa, "x^{a}");
        }
    }
}
