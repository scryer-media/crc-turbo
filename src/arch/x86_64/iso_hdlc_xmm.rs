// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.

//! CRC-32/ISO-HDLC in libdeflate 1.19's shape for 16 bytes up to 1 KiB: 128-bit vectors only,
//! in the AVX encoding, with three coefficient pairs held in registers. Below 64 bytes one
//! accumulator takes each 16-byte block in turn; from 64 bytes four accumulators step 64 bytes
//! at a time, then fold four onto two and two onto one, each halving taking the next whole
//! piece of the buffer the length's bits call for, then the last bytes by an overlapping read
//! and the 96-bit Barrett reduction.
//!
//! libdeflate (`lib/x86/crc32_pclmul_template.h` in 1.19, MIT) runs this shape as
//! `crc32_pclmul_avx` below 1 KiB on every CPU with PCLMULQDQ and AVX, the distribution
//! package's build included, whose disassembly this follows.

#![cfg(target_arch = "x86_64")]

use core::arch::x86_64::*;

use crate::arch::x86::sse::X86SsePclmulqdqOps;
use crate::structs::Width32;
use crate::traits::EnhancedCrcWidth;

/// CRC-32/ISO-HDLC's coefficient pair for folding a 16-byte block `distance` blocks forward,
/// low key first: `x^(128 * distance - 32)` and `x^(128 * distance + 32)` mod P, reflected and
/// shifted left by one, the pairs of the generated key table.
const fn pair(distance: usize) -> [u64; 2] {
    use crate::crc32::powers::x_to_the;
    let poly = (crate::crc32::consts::CRC32_ISO_HDLC.poly as u32).reverse_bits();
    let bits = 128 * distance as u64;
    [
        (x_to_the(bits - 32, poly) as u64) << 1,
        (x_to_the(bits + 32, poly) as u64) << 1,
    ]
}

/// The pairs for one, two and four blocks.
const PAIRS: [[u64; 2]; 3] = [pair(1), pair(2), pair(4)];

/// The pairs are the generated table's: keys 1 and 2 fold one block, 19 and 20 two, 15 and 16
/// four.
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
unsafe fn key(p: [u64; 2]) -> __m128i {
    _mm_set_epi64x(p[1] as i64, p[0] as i64)
}

/// `a` folded forward by `k`'s distance onto `b`.
#[inline(always)]
unsafe fn fold(a: __m128i, b: __m128i, k: __m128i) -> __m128i {
    _mm_xor_si128(
        _mm_xor_si128(
            _mm_clmulepi64_si128(a, k, 0x10),
            _mm_clmulepi64_si128(a, k, 0x01),
        ),
        b,
    )
}

#[inline(always)]
unsafe fn load(p: *const u8) -> __m128i {
    _mm_loadu_si128(p as *const __m128i)
}

/// The published CRC-32/ISO-HDLC of `bytes` after `crc`, the published CRC so far, widened as
/// the tiers' `iso_hdlc_mid` return it.
///
/// # Safety
///
/// The CPU must have AVX, SSE4.1 and PCLMULQDQ, and `bytes` must be at least 16 bytes long.
#[inline]
#[target_feature(enable = "sse4.1,pclmulqdq,avx")]
pub(crate) unsafe fn iso_hdlc_xmm(crc: u32, bytes: &[u8]) -> u64 {
    debug_assert!(bytes.len() >= 16);
    let ops = X86SsePclmulqdqOps;
    let len = bytes.len();
    let mut p = bytes.as_ptr();
    let end = p.add(len);
    let k1 = key(PAIRS[0]);
    let mut x0 = _mm_xor_si128(load(p), Width32::create_state(!crc, true, &ops).value);
    if len < 64 {
        p = p.add(16);
        if len >= 32 {
            x0 = fold(x0, load(p), k1);
            p = p.add(16);
            if len >= 48 {
                x0 = fold(x0, load(p), k1);
                p = p.add(16);
            }
        }
    } else {
        let mut x1 = load(p.add(16));
        let mut x2 = load(p.add(32));
        let mut x3 = load(p.add(48));
        p = p.add(64);
        if len >= 128 {
            let k4 = key(PAIRS[2]);
            while end.offset_from(p) >= 64 {
                x0 = fold(x0, load(p), k4);
                x1 = fold(x1, load(p.add(16)), k4);
                x2 = fold(x2, load(p.add(32)), k4);
                x3 = fold(x3, load(p.add(48)), k4);
                p = p.add(64);
            }
        }
        let k2 = key(PAIRS[1]);
        x0 = fold(x0, x2, k2);
        x1 = fold(x1, x3, k2);
        if len & 32 != 0 {
            x0 = fold(x0, load(p), k2);
            x1 = fold(x1, load(p.add(16)), k2);
            p = p.add(32);
        }
        x0 = fold(x0, x1, k1);
        if len & 16 != 0 {
            x0 = fold(x0, load(p), k1);
            p = p.add(16);
        }
    }
    let rest = len & 15;
    if rest != 0 {
        // The last 16 bytes of the buffer, overlapping the block before them, and the state
        // split around the bytes that are new, as `algorithm::get_last_two_xmms` merges them
        let (table, offset) = Width32::get_last_bytes_table_ptr(true, rest);
        let last = load(p.add(rest).sub(16));
        let control = load(table.add(offset));
        let shuffled = _mm_shuffle_epi8(x0, control);
        let flipped = _mm_xor_si128(control, _mm_set1_epi8(0x80u8 as i8));
        let head = _mm_shuffle_epi8(x0, flipped);
        let merged = _mm_blendv_epi8(last, shuffled, flipped);
        x0 = fold(head, merged, k1);
    }
    let keys = crate::crc32::consts::CRC32_ISO_HDLC.keys.fold_keys();
    !Width32::perform_final_reduction(x0, true, keys, &ops) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every length from 16 to 1100, from four starts, from the initial CRC and a carried one,
    /// forced onto the kernel, against the software path.
    #[test]
    fn xmm_tier_matches_software() {
        if !(is_x86_feature_detected!("avx")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1"))
        {
            eprintln!(
                "skipping the CRC-32/ISO-HDLC 128-bit kernel: the CPU lacks PCLMULQDQ or AVX"
            );
            return;
        }
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let buf: Vec<u8> = (0..1200u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let lens: Vec<usize> = if cfg!(miri) {
            vec![16, 17, 32, 47, 63, 64, 100, 128, 200, 256, 300, 513]
        } else {
            (16..=1100).collect()
        };
        for &len in &lens {
            for off in [0usize, 1, 15, 33] {
                let data = &buf[off..off + len];
                for crc in [0u32, 0x1234_5678] {
                    let want = !(crate::arch::software::update(!crc as u64, data, params) as u32);
                    let got = unsafe { iso_hdlc_xmm(crc, data) } as u32;
                    assert_eq!(got, want, "len {len} off {off} crc {crc:#x}");
                }
            }
        }
    }
}
