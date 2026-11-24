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

/// [`P16`] as fast-crc32's `v4` lays out its loop coefficient: the key for the low halves'
/// product in the low word, so that one register serves both products, `0x00` and `0x11`.
const V4_K: [u64; 2] = [P16[1], P16[0]];

/// The four 512-bit accumulators folded over every whole 256-byte step of the `rest` bytes at
/// `p`, in the instruction order gcc 11 (`-O3`, the Amazon Linux 2023 build) gives fast-crc32's
/// `avx512_vpclmulqdq_crc32_v4` loop (Corsix, MIT): the four low products against one
/// coefficient register with the pointer step among them, the four high products, the four
/// three-way XORs with the step's loads behind the pointer, and a compare against the end.
/// Returns the accumulators, the pointer past the last step and the bytes left, fewer than 256.
///
/// # Safety
///
/// The CPU must have AVX-512F and VPCLMULQDQ, and `rest` bytes must be readable from `p`.
#[inline]
#[target_feature(enable = "avx512f,avx512vl,avx512bw,vpclmulqdq")]
unsafe fn v4_loop(v: [__m512i; 4], p: *const u8, rest: usize) -> ([__m512i; 4], *const u8, usize) {
    let k = key512(V4_K);
    // Miri runs the same loop from intrinsics.
    #[cfg(miri)]
    {
        let [mut v0, mut v1, mut v2, mut v3] = v;
        let (mut p, mut rest) = (p, rest);
        while rest >= 256 {
            let y0 = _mm512_clmulepi64_epi128(v0, k, 0x00);
            v0 = _mm512_clmulepi64_epi128(v0, k, 0x11);
            let y1 = _mm512_clmulepi64_epi128(v1, k, 0x00);
            v1 = _mm512_clmulepi64_epi128(v1, k, 0x11);
            let y2 = _mm512_clmulepi64_epi128(v2, k, 0x00);
            v2 = _mm512_clmulepi64_epi128(v2, k, 0x11);
            let y3 = _mm512_clmulepi64_epi128(v3, k, 0x00);
            v3 = _mm512_clmulepi64_epi128(v3, k, 0x11);
            v0 = _mm512_ternarylogic_epi64(v0, y0, load512(p), 0x96);
            v1 = _mm512_ternarylogic_epi64(v1, y1, load512(p.add(64)), 0x96);
            v2 = _mm512_ternarylogic_epi64(v2, y2, load512(p.add(128)), 0x96);
            v3 = _mm512_ternarylogic_epi64(v3, y3, load512(p.add(192)), 0x96);
            p = p.add(256);
            rest -= 256;
        }
        ([v0, v1, v2, v3], p, rest)
    }
    #[cfg(not(miri))]
    {
        let [mut v0, mut v1, mut v2, mut v3] = v;
        let (mut p, mut rest) = (p, rest);
        // SAFETY: each step reads the 256 bytes at `p`, and runs only while `rest`, the bytes
        // readable from `p`, is at least 256.
        core::arch::asm!(
            "cmp {n}, 255",
            "jbe 3f",
            "mov {e}, {n}",
            "and {e}, -256",
            "add {e}, {p}",
            "and {n}, 255",
            ".p2align 4",
            "2:",
            "vpclmulqdq {y0}, {v0}, {k}, 0x00",
            "add {p}, 256",
            "vpclmulqdq {y1}, {v1}, {k}, 0x00",
            "vpclmulqdq {y2}, {v2}, {k}, 0x00",
            "vpclmulqdq {y3}, {v3}, {k}, 0x00",
            "vpclmulqdq {v0}, {v0}, {k}, 0x11",
            "vpclmulqdq {v1}, {v1}, {k}, 0x11",
            "vpclmulqdq {v2}, {v2}, {k}, 0x11",
            "vpclmulqdq {v3}, {v3}, {k}, 0x11",
            "vpternlogq {v0}, {y0}, zmmword ptr [{p} - 256], 0x96",
            "vpternlogq {v1}, {y1}, zmmword ptr [{p} - 192], 0x96",
            "vpternlogq {v2}, {y2}, zmmword ptr [{p} - 128], 0x96",
            "vpternlogq {v3}, {y3}, zmmword ptr [{p} - 64], 0x96",
            "cmp {p}, {e}",
            "jne 2b",
            "3:",
            v0 = inout(zmm_reg) v0,
            v1 = inout(zmm_reg) v1,
            v2 = inout(zmm_reg) v2,
            v3 = inout(zmm_reg) v3,
            y0 = out(zmm_reg) _,
            y1 = out(zmm_reg) _,
            y2 = out(zmm_reg) _,
            y3 = out(zmm_reg) _,
            k = in(zmm_reg) k,
            p = inout(reg) p,
            n = inout(reg) rest,
            e = out(reg) _,
            options(pure, readonly, nostack),
        );
        ([v0, v1, v2, v3], p, rest)
    }
}

/// The published CRC-32/ISO-HDLC of `bytes` after `crc`, the published CRC so far, widened as
/// the tier's `iso_hdlc_mid` returns it. With `EIGHT`, eight accumulators over 512-byte steps,
/// otherwise four over 256-byte steps. With `V4_ASM` (and not `EIGHT`) the four-accumulator
/// loop is [`v4_loop`], inline assembly in the instruction order of fast-crc32's
/// `avx512_vpclmulqdq_crc32_v4`, rather than the order the compiler picks.
///
/// # Safety
///
/// The CPU must have AVX-512VL, AVX-512BW, VPCLMULQDQ, PCLMULQDQ and SSE4.1, and `bytes` must be
/// at least 512 bytes long with `EIGHT`, 256 without.
#[inline(never)]
#[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq,pclmulqdq,sse4.1")]
pub(crate) unsafe fn iso_hdlc_fold512<const EIGHT: bool>(crc: u32, bytes: &[u8]) -> u64 {
    fold512_body::<EIGHT, false>(crc, bytes)
}

/// [`iso_hdlc_fold512`] with four accumulators and [`v4_loop`], for Ice Lake servers: a
/// function of its own so that `cargo xtask codegen` holds its assembly.
///
/// # Safety
///
/// As [`iso_hdlc_fold512`] without `EIGHT`.
#[inline(never)]
#[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq,pclmulqdq,sse4.1")]
pub(crate) unsafe fn iso_hdlc_fold512_v4(crc: u32, bytes: &[u8]) -> u64 {
    fold512_body::<false, true>(crc, bytes)
}

/// The body of [`iso_hdlc_fold512`] and [`iso_hdlc_fold512_v4`], each of which is its only caller.
#[inline]
#[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq,pclmulqdq,sse4.1")]
unsafe fn fold512_body<const EIGHT: bool, const V4_ASM: bool>(crc: u32, bytes: &[u8]) -> u64 {
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
    } else if V4_ASM {
        p = p.add(256);
        let after = v4_loop([v0, v1, v2, v3], p, left - 256);
        [v0, v1, v2, v3] = after.0;
        p = after.1;
        left = after.2 + 256;
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
    /// initial CRC and a carried one, forced onto the three shapes, against the software path.
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
                    let got = unsafe { iso_hdlc_fold512_v4(crc, data) } as u32;
                    assert_eq!(got, want, "v4 asm, len {len} off {off} crc {crc:#x}");
                    if len >= 512 {
                        let got = unsafe { iso_hdlc_fold512::<true>(crc, data) } as u32;
                        assert_eq!(got, want, "eight, len {len} off {off} crc {crc:#x}");
                    }
                }
            }
        }
    }

    /// Ice Lake servers' checksum entry, forced onto it, against the software path over every
    /// length from 0 to 4200 from three starts: the ladder, the tier's mid fold, `v4`'s loop from
    /// 1 KiB and the long fold from 4 KiB.
    #[test]
    #[cfg(feature = "std")]
    #[cfg_attr(miri, ignore)]
    fn ice_lake_entry_tier_matches_software() {
        if !(is_x86_feature_detected!("avx512vl")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("vpclmulqdq")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1"))
        {
            eprintln!(
                "skipping Ice Lake's CRC-32/ISO-HDLC entry: the CPU lacks AVX-512 VPCLMULQDQ"
            );
            return;
        }
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let buf: Vec<u8> = (0..4300u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        for len in 0..=4200 {
            for off in [0usize, 7, 33] {
                let data = &buf[off..off + len];
                let want = !(crate::arch::software::update(!0, data, params) as u32);
                let got = unsafe { crate::arch::x86::short_asm::iso_hdlc_checksum_avx512_icl(data) }
                    as u32;
                assert_eq!(got, want, "checksum len {len} off {off}");
            }
        }
    }
}
