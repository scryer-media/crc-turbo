// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.

//! CRC-32/ISO-HDLC in libdeflate's shape for the AVX2 tier with VPCLMULQDQ: 256-bit vectors,
//! eight accumulators over 256-byte steps, then a cascade that folds four vectors onto four,
//! two onto two and one onto one, each halving taking the next whole piece of the buffer the
//! length's bits call for, then 256 bits onto 128, a 16-byte block, the last bytes by an
//! overlapping read, and the 96-bit Barrett reduction. Each fold is onto the accumulator a
//! fixed distance ahead rather than onto the last block, so every step uses one of five
//! coefficient pairs.
//!
//! libdeflate (`lib/x86/crc32_pclmul_template.h`, MIT) runs this shape as
//! `crc32_x86_vpclmulqdq_avx2` on CPUs with AVX2 and VPCLMULQDQ but no AVX-512. On a Xeon 6731E
//! (Sierra Forest), whose E-cores split each 256-bit operation in two, it was ahead of this
//! tier's own folds from 96 bytes to 1 KiB, so Intel's E-core parts take it there; see
//! `crate::arch::choose_iso_hdlc`.

#![cfg(target_arch = "x86_64")]

use core::arch::x86_64::*;

use crate::arch::x86::sse::X86SsePclmulqdqOps;
use crate::structs::Width32;
use crate::traits::EnhancedCrcWidth;

use super::iso_hdlc_fold512::pair;

/// The pairs for one, two, four, eight and sixteen blocks: a 16-byte block, and one, two, four
/// and eight 32-byte vectors.
const PAIRS: [[u64; 2]; 5] = [pair(1), pair(2), pair(4), pair(8), pair(16)];

/// The pairs are the generated table's where it has them: keys 1 and 2 fold one block, 19 and
/// 20 two, 15 and 16 four.
const _: () = {
    let keys = match crate::crc32::consts::CRC32_ISO_HDLC.keys {
        crate::CrcKeysStorage::KeysFold256(keys) => keys,
        crate::CrcKeysStorage::KeysFutureTest(_) => panic!("CRC-32/ISO-HDLC has 23 keys"),
    };
    assert!(PAIRS[0][0] == keys[1] && PAIRS[0][1] == keys[2]);
    assert!(PAIRS[1][0] == keys[19] && PAIRS[1][1] == keys[20]);
    assert!(PAIRS[2][0] == keys[15] && PAIRS[2][1] == keys[16]);
};

#[inline(always)]
unsafe fn key128(p: [u64; 2]) -> __m128i {
    _mm_set_epi64x(p[1] as i64, p[0] as i64)
}

#[inline(always)]
unsafe fn key256(p: [u64; 2]) -> __m256i {
    _mm256_set_epi64x(p[1] as i64, p[0] as i64, p[1] as i64, p[0] as i64)
}

/// `a` folded forward by `k`'s distance onto `b`, in each 128-bit lane.
#[inline(always)]
unsafe fn fold256(a: __m256i, b: __m256i, k: __m256i) -> __m256i {
    _mm256_xor_si256(
        _mm256_xor_si256(
            _mm256_clmulepi64_epi128(a, k, 0x10),
            _mm256_clmulepi64_epi128(a, k, 0x01),
        ),
        b,
    )
}

/// `a` folded forward one block onto `b`.
#[inline(always)]
unsafe fn fold128(a: __m128i, b: __m128i, k: __m128i) -> __m128i {
    _mm_xor_si128(
        _mm_xor_si128(
            _mm_clmulepi64_si128(a, k, 0x10),
            _mm_clmulepi64_si128(a, k, 0x01),
        ),
        b,
    )
}

#[inline(always)]
unsafe fn load256(p: *const u8) -> __m256i {
    _mm256_loadu_si256(p as *const __m256i)
}

/// The published CRC-32/ISO-HDLC of `bytes` after `crc`, the published CRC so far, widened as
/// the tier's `iso_hdlc_mid` returns it, whose place this takes on Intel's E-core parts.
///
/// # Safety
///
/// The CPU must have AVX2, VPCLMULQDQ, SSE4.1 and PCLMULQDQ, and `bytes` must be at least 32
/// bytes long.
#[inline(never)]
#[target_feature(enable = "avx2,vpclmulqdq,sse4.1,pclmulqdq")]
pub(crate) unsafe fn iso_hdlc_cascade(crc: u32, bytes: &[u8]) -> u64 {
    debug_assert!(bytes.len() >= 32);
    let ops = X86SsePclmulqdqOps;
    let params = &crate::crc32::consts::CRC32_ISO_HDLC;
    let len = bytes.len();
    let mut p = bytes.as_ptr();
    let mut left = len;
    let init = _mm256_zextsi128_si256(Width32::create_state(!crc, true, &ops).value);
    let k1v = key256(PAIRS[1]);

    let (mut v0, mut v1): (__m256i, __m256i);
    'two: {
        'one: {
            if len < 256 {
                v0 = _mm256_xor_si256(load256(p), init);
                if len < 64 {
                    p = p.add(32);
                    break 'one;
                }
                v1 = load256(p.add(32));
                if len < 128 {
                    p = p.add(64);
                    break 'two;
                }
                let k2v = key256(PAIRS[2]);
                let v2 = load256(p.add(64));
                let v3 = load256(p.add(96));
                p = p.add(128);
                v0 = fold256(v0, v2, k2v);
                v1 = fold256(v1, v3, k2v);
            } else {
                v0 = _mm256_xor_si256(load256(p), init);
                v1 = load256(p.add(32));
                let mut v2 = load256(p.add(64));
                let mut v3 = load256(p.add(96));
                let mut v4 = load256(p.add(128));
                let mut v5 = load256(p.add(160));
                let mut v6 = load256(p.add(192));
                let mut v7 = load256(p.add(224));
                p = p.add(256);
                let k8v = key256(PAIRS[4]);
                while left >= 512 {
                    v0 = fold256(v0, load256(p), k8v);
                    v1 = fold256(v1, load256(p.add(32)), k8v);
                    v2 = fold256(v2, load256(p.add(64)), k8v);
                    v3 = fold256(v3, load256(p.add(96)), k8v);
                    v4 = fold256(v4, load256(p.add(128)), k8v);
                    v5 = fold256(v5, load256(p.add(160)), k8v);
                    v6 = fold256(v6, load256(p.add(192)), k8v);
                    v7 = fold256(v7, load256(p.add(224)), k8v);
                    p = p.add(256);
                    left -= 256;
                }
                let k4v = key256(PAIRS[3]);
                v0 = fold256(v0, v4, k4v);
                v1 = fold256(v1, v5, k4v);
                v2 = fold256(v2, v6, k4v);
                v3 = fold256(v3, v7, k4v);
                if left & 128 != 0 {
                    v0 = fold256(v0, load256(p), k4v);
                    v1 = fold256(v1, load256(p.add(32)), k4v);
                    v2 = fold256(v2, load256(p.add(64)), k4v);
                    v3 = fold256(v3, load256(p.add(96)), k4v);
                    p = p.add(128);
                }
                let k2v = key256(PAIRS[2]);
                v0 = fold256(v0, v2, k2v);
                v1 = fold256(v1, v3, k2v);
            }
            // Fewer than 128 bytes left
            if left & 64 != 0 {
                let k2v = key256(PAIRS[2]);
                v0 = fold256(v0, load256(p), k2v);
                v1 = fold256(v1, load256(p.add(32)), k2v);
                p = p.add(64);
            }
            break 'two;
        }
        // One vector, fewer than 32 bytes left
        return finish(v0, p, left, &ops, params);
    }
    // Two vectors, fewer than 64 bytes left
    v0 = fold256(v0, v1, k1v);
    if left & 32 != 0 {
        v0 = fold256(v0, load256(p), k1v);
        p = p.add(32);
    }
    finish(v0, p, left, &ops, params)
}

/// The last vector `v0` onto 128 bits, the block and the bytes after it, `left & 31` of them
/// at `p`, and the reduction to the published CRC.
#[inline(always)]
unsafe fn finish(
    v0: __m256i,
    p: *const u8,
    left: usize,
    ops: &X86SsePclmulqdqOps,
    params: &crate::CrcParams,
) -> u64 {
    let k1 = key128(PAIRS[0]);
    let mut p = p;
    let mut x = fold128(
        _mm256_castsi256_si128(v0),
        _mm256_extracti128_si256(v0, 1),
        k1,
    );
    if left & 16 != 0 {
        x = fold128(x, _mm_loadu_si128(p as *const __m128i), k1);
        p = p.add(16);
    }
    let rest = left & 15;
    if rest != 0 {
        // The last 16 bytes of the buffer, overlapping the block before them, and the state
        // split around the bytes that are new, as `algorithm::get_last_two_xmms` merges them
        let (table, offset) = Width32::get_last_bytes_table_ptr(true, rest);
        let last = _mm_loadu_si128(p.add(rest).sub(16) as *const __m128i);
        let control = _mm_loadu_si128(table.add(offset) as *const __m128i);
        let shuffled = _mm_shuffle_epi8(x, control);
        let flipped = _mm_xor_si128(control, _mm_set1_epi8(0x80u8 as i8));
        let head = _mm_shuffle_epi8(x, flipped);
        let merged = _mm_blendv_epi8(last, shuffled, flipped);
        x = fold128(head, merged, k1);
    }
    let keys = params.keys.fold_keys();
    !Width32::perform_final_reduction(x, true, keys, ops) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every length from 32 to 1100, from four starts, from the initial CRC and a carried one,
    /// forced onto the kernel, against the software path.
    #[test]
    fn cascade_matches_software() {
        if !(is_x86_feature_detected!("avx2")
            && is_x86_feature_detected!("vpclmulqdq")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1"))
        {
            eprintln!("skipping the CRC-32/ISO-HDLC cascade: the CPU lacks VPCLMULQDQ or AVX2");
            return;
        }
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let buf: Vec<u8> = (0..1200u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let lens: Vec<usize> = if cfg!(miri) {
            vec![32, 47, 64, 100, 128, 200, 256, 300, 513]
        } else {
            (32..=1100).collect()
        };
        for &len in &lens {
            for off in [0usize, 1, 15, 33] {
                let data = &buf[off..off + len];
                for crc in [0u32, 0x1234_5678] {
                    let want = !(crate::arch::software::update(!crc as u64, data, params) as u32);
                    let got = unsafe { iso_hdlc_cascade(crc, data) } as u32;
                    assert_eq!(got, want, "len {len} off {off} crc {crc:#x}");
                }
            }
        }
    }
}
