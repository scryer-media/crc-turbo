// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.

//! CRC-32/ISO-HDLC in 512-bit registers for the AVX-512 tier with VPCLMULQDQ, in the shape of
//! the kernels that beat its general fold from 1 KiB to 8 KiB: four or eight 512-bit
//! accumulators over steps of their whole width, fast-crc32's `avx512_vpclmulqdq_crc32_v4` and
//! `_v8`, and libdeflate's `crc32_x86_vpclmulqdq_avx512_vl512`. After the loop a cascade folds
//! the accumulators in halves, eight onto four, four onto two and two onto one, each halving
//! taking the next whole piece of the buffer the length's bits call for, as libdeflate's
//! template (`lib/x86/crc32_pclmul_template.h`, MIT) does, then the last register's four lanes
//! onto its last in one multiply, the 16-byte blocks left, the last bytes by an overlapping
//! read, and the 96-bit Barrett reduction.

#![cfg(target_arch = "x86_64")]

use core::arch::x86_64::*;

use crate::arch::x86::sse::X86SsePclmulqdqOps;
use crate::structs::Width32;
use crate::traits::EnhancedCrcWidth;

/// CRC-32/ISO-HDLC's coefficient pair for folding a 16-byte block `distance` blocks forward,
/// low key first: `x^(128 * distance - 32)` and `x^(128 * distance + 32)` mod P, reflected and
/// shifted left by one, the pairs of the generated key table.
pub(crate) const fn pair(distance: usize) -> [u64; 2] {
    use crate::crc32::powers::x_to_the;
    let poly = (crate::crc32::consts::CRC32_ISO_HDLC.poly as u32).reverse_bits();
    let bits = 128 * distance as u64;
    [
        (x_to_the(bits - 32, poly) as u64) << 1,
        (x_to_the(bits + 32, poly) as u64) << 1,
    ]
}

/// The pairs for one, two and three blocks, the lanes of a 512-bit register.
const P1: [u64; 2] = pair(1);
const P2: [u64; 2] = pair(2);
const P3: [u64; 2] = pair(3);
/// One, two, four and eight 64-byte registers.
const P4: [u64; 2] = pair(4);
const P8: [u64; 2] = pair(8);
const P16: [u64; 2] = pair(16);
const P32: [u64; 2] = pair(32);

/// The pairs are the generated table's where it has them.
const _: () = {
    let keys = match crate::crc32::consts::CRC32_ISO_HDLC.keys {
        crate::CrcKeysStorage::KeysFold256(keys) => keys,
        crate::CrcKeysStorage::KeysFutureTest(_) => panic!("CRC-32/ISO-HDLC has 23 keys"),
    };
    assert!(P1[0] == keys[1] && P1[1] == keys[2]);
    assert!(P2[0] == keys[19] && P2[1] == keys[20]);
    assert!(P3[0] == keys[17] && P3[1] == keys[18]);
    assert!(P4[0] == keys[15] && P4[1] == keys[16]);
    assert!(P8[0] == keys[3] && P8[1] == keys[4]);
    assert!(P16[0] == keys[21] && P16[1] == keys[22]);
};

#[inline(always)]
unsafe fn key128(p: [u64; 2]) -> __m128i {
    _mm_set_epi64x(p[1] as i64, p[0] as i64)
}

#[inline(always)]
unsafe fn key512(p: [u64; 2]) -> __m512i {
    _mm512_broadcast_i32x4(key128(p))
}

/// `a` folded forward by `k`'s distance onto `b`, in each 128-bit lane.
#[inline(always)]
unsafe fn fold512(a: __m512i, b: __m512i, k: __m512i) -> __m512i {
    _mm512_ternarylogic_epi64(
        _mm512_clmulepi64_epi128(a, k, 0x10),
        _mm512_clmulepi64_epi128(a, k, 0x01),
        b,
        0x96,
    )
}

/// `a` folded forward one block onto `b`.
#[inline(always)]
unsafe fn fold128(a: __m128i, b: __m128i, k: __m128i) -> __m128i {
    _mm_ternarylogic_epi64(
        _mm_clmulepi64_si128(a, k, 0x10),
        _mm_clmulepi64_si128(a, k, 0x01),
        b,
        0x96,
    )
}

#[inline(always)]
unsafe fn load512(p: *const u8) -> __m512i {
    _mm512_loadu_si512(p as *const __m512i)
}

/// The published CRC-32/ISO-HDLC of `bytes` after `crc`, the published CRC so far, widened as
/// the tier's `iso_hdlc_mid` returns it. With `EIGHT`, eight accumulators over 512-byte steps,
/// otherwise four over 256-byte steps.
///
/// # Safety
///
/// The CPU must have AVX-512VL, AVX-512BW, VPCLMULQDQ, PCLMULQDQ and SSE4.1, and `bytes` must be
/// at least 512 bytes long with `EIGHT`, 256 without.
#[inline(never)]
#[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq,pclmulqdq,sse4.1")]
pub(crate) unsafe fn iso_hdlc_fold512<const EIGHT: bool>(crc: u32, bytes: &[u8]) -> u64 {
    let len = bytes.len();
    debug_assert!(len >= if EIGHT { 512 } else { 256 });
    let ops = X86SsePclmulqdqOps;
    let params = &crate::crc32::consts::CRC32_ISO_HDLC;
    let mut p = bytes.as_ptr();
    // The bytes from `p` on, plus those of the accumulators' last step
    let mut left = len;
    let init = _mm512_zextsi128_si512(Width32::create_state(!crc, true, &ops).value);

    let mut v0 = _mm512_xor_si512(load512(p), init);
    let mut v1 = load512(p.add(64));
    let mut v2 = load512(p.add(128));
    let mut v3 = load512(p.add(192));
    if EIGHT {
        let mut v4 = load512(p.add(256));
        let mut v5 = load512(p.add(320));
        let mut v6 = load512(p.add(384));
        let mut v7 = load512(p.add(448));
        p = p.add(512);
        let k8 = key512(P32);
        while left >= 1024 {
            v0 = fold512(v0, load512(p), k8);
            v1 = fold512(v1, load512(p.add(64)), k8);
            v2 = fold512(v2, load512(p.add(128)), k8);
            v3 = fold512(v3, load512(p.add(192)), k8);
            v4 = fold512(v4, load512(p.add(256)), k8);
            v5 = fold512(v5, load512(p.add(320)), k8);
            v6 = fold512(v6, load512(p.add(384)), k8);
            v7 = fold512(v7, load512(p.add(448)), k8);
            p = p.add(512);
            left -= 512;
        }
        let k4 = key512(P16);
        v0 = fold512(v0, v4, k4);
        v1 = fold512(v1, v5, k4);
        v2 = fold512(v2, v6, k4);
        v3 = fold512(v3, v7, k4);
        if left & 256 != 0 {
            v0 = fold512(v0, load512(p), k4);
            v1 = fold512(v1, load512(p.add(64)), k4);
            v2 = fold512(v2, load512(p.add(128)), k4);
            v3 = fold512(v3, load512(p.add(192)), k4);
            p = p.add(256);
        }
    } else {
        p = p.add(256);
        let k4 = key512(P16);
        while left >= 512 {
            v0 = fold512(v0, load512(p), k4);
            v1 = fold512(v1, load512(p.add(64)), k4);
            v2 = fold512(v2, load512(p.add(128)), k4);
            v3 = fold512(v3, load512(p.add(192)), k4);
            p = p.add(256);
            left -= 256;
        }
    }
    // Fewer than 256 bytes left
    let k2 = key512(P8);
    v0 = fold512(v0, v2, k2);
    v1 = fold512(v1, v3, k2);
    if left & 128 != 0 {
        v0 = fold512(v0, load512(p), k2);
        v1 = fold512(v1, load512(p.add(64)), k2);
        p = p.add(128);
    }
    let k1 = key512(P4);
    v0 = fold512(v0, v1, k1);
    if left & 64 != 0 {
        v0 = fold512(v0, load512(p), k1);
        p = p.add(64);
    }

    // The four lanes onto the last, the first three by 48, 32 and 16 bytes, the last by none
    let lane_keys = _mm512_set_epi64(
        0,
        0,
        P1[1] as i64,
        P1[0] as i64,
        P2[1] as i64,
        P2[0] as i64,
        P3[1] as i64,
        P3[0] as i64,
    );
    let lanes = _mm512_ternarylogic_epi64(
        _mm512_clmulepi64_epi128(v0, lane_keys, 0x10),
        _mm512_clmulepi64_epi128(v0, lane_keys, 0x01),
        _mm512_maskz_mov_epi64(0b1100_0000, v0),
        0x96,
    );
    let halves = _mm256_xor_si256(
        _mm512_castsi512_si256(lanes),
        _mm512_extracti64x4_epi64(lanes, 1),
    );
    let mut x = _mm_xor_si128(
        _mm256_castsi256_si128(halves),
        _mm256_extracti128_si256(halves, 1),
    );

    // Fewer than 64 bytes left
    let k1 = key128(P1);
    if left & 32 != 0 {
        x = fold128(x, _mm_loadu_si128(p as *const __m128i), k1);
        x = fold128(x, _mm_loadu_si128(p.add(16) as *const __m128i), k1);
        p = p.add(32);
    }
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
    !Width32::perform_final_reduction(x, true, keys, &ops) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every length from 256 to 2200 and the step boundaries above, from five starts, from the
    /// initial CRC and a carried one, forced onto both shapes, against the software path.
    #[test]
    #[cfg(feature = "std")]
    #[cfg_attr(miri, ignore)]
    fn iso_hdlc_fold512_tier_matches_software() {
        if !(is_x86_feature_detected!("avx512vl")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("vpclmulqdq")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1"))
        {
            eprintln!(
                "skipping the 512-bit CRC-32/ISO-HDLC fold: the CPU lacks AVX-512 VPCLMULQDQ"
            );
            return;
        }
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let buf: Vec<u8> = (0..17000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let mut lens: Vec<usize> = (256..=2200).collect();
        lens.extend([
            3071, 3072, 3073, 4095, 4096, 4097, 8191, 8192, 8193, 16383, 16384,
        ]);
        for &len in &lens {
            for off in [0usize, 1, 15, 33, 63] {
                let data = &buf[off..off + len];
                for crc in [0u32, 0x1234_5678] {
                    let want = !(crate::arch::software::update(!crc as u64, data, params) as u32);
                    let got = unsafe { iso_hdlc_fold512::<false>(crc, data) } as u32;
                    assert_eq!(got, want, "four, len {len} off {off} crc {crc:#x}");
                    if len >= 512 {
                        let got = unsafe { iso_hdlc_fold512::<true>(crc, data) } as u32;
                        assert_eq!(got, want, "eight, len {len} off {off} crc {crc:#x}");
                    }
                }
            }
        }
    }
}
