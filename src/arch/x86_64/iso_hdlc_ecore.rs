// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.
//
// The assembly in this file follows the compiled code of libdeflate's `crc32_pclmul_avx`, which
// carries this notice:
//
// Copyright 2016 Eric Biggers
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software
// and associated documentation files (the "Software"), to deal in the Software without
// restriction, including without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all copies or
// substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
// BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! CRC-32/ISO-HDLC's checksum entry on Intel's E-core parts (Alder Lake-N, Sierra Forest, Grand
//! Ridge, Clearwater Forest), from 16 bytes up to [`ASM_END`], as inline assembly that follows
//! the compiled code of libdeflate's `crc32_pclmul_avx` (libdeflate 1.19,
//! `lib/x86/crc32_pclmul_template.h`, MIT, Copyright 2016 Eric Biggers), as the Ubuntu 24.04
//! package builds it: 128-bit registers only, in the AVX encoding. Below 64 bytes one
//! accumulator takes each 16-byte block in turn; from 64 bytes four accumulators step 64 bytes
//! at a time, then fold four onto two and two onto one, each halving taking the next whole
//! piece of the buffer the length's bits call for; the bytes after the last whole block are
//! merged by an overlapping read and libdeflate's 48-byte shift table. The instruction order,
//! the register use, the `lqlq`/`hqhq` products against byte-swapped coefficient pairs and the
//! loop bound are its. Two departures: the reduction is this crate's three-multiply 96-bit
//! Barrett step, one multiply fewer than libdeflate 1.19's; and the blocks are laid out in a
//! different order, the 16 to 63 byte path first and falling through from the length test,
//! then the 64-byte path falling straight into the tail, so 16 to 47 bytes take one taken
//! branch fewer and no path takes one more.
//!
//! These cores have no micro-op cache and split each 256-bit operation in two, and the
//! compiled form of the same shape from intrinsics, entered from the tier's ladder, measured
//! 0.76x to 0.89x of libdeflate from 48 to 192 bytes on a Xeon 6731E; the entry here is the
//! whole function, with no test of the ladder ahead of it and one length test to the 128-bit
//! fold at its end.

#![cfg(target_arch = "x86_64")]

#[cfg(not(miri))]
use core::arch::asm;

/// Where the entry hands over to the 128-bit fold, [`crate::arch::iso_hdlc_mid_x86_sse_avx`].
pub(crate) const ASM_END: usize = 1024;

/// The loop takes at most 1023 bytes, libdeflate's own limit for this shape.
const _: () = assert!(ASM_END > 64 && ASM_END <= 1024);

/// CRC-32/ISO-HDLC's coefficient pair for folding a 16-byte block `distance` blocks forward, in
/// the order libdeflate's `lqlq`/`hqhq` products take it: `x^(128 * distance + 32)` and then
/// `x^(128 * distance - 32)` mod P, reflected and shifted left by one. The generated table holds
/// the same pairs low key first.
const fn pair(distance: usize) -> [u64; 2] {
    use crate::crc32::powers::x_to_the;
    let poly = (crate::crc32::consts::CRC32_ISO_HDLC.poly as u32).reverse_bits();
    let bits = 128 * distance as u64;
    [
        (x_to_the(bits + 32, poly) as u64) << 1,
        (x_to_the(bits - 32, poly) as u64) << 1,
    ]
}

/// The entry's constants, one 64-byte line for the coefficient pairs and the reduction, and
/// libdeflate's shift table after them.
#[cfg(not(miri))]
#[repr(C, align(64))]
struct Consts {
    k4: [u64; 2],
    k2: [u64; 2],
    k1: [u64; 2],
    /// `x^96 mod P` in the low word, for the fold of 128 bits to 96.
    fold96: [u64; 2],
    /// The 96-bit Barrett quotient constant in the low word and P in the high.
    barrett: [u64; 2],
    _pad: [u64; 2],
    /// Sixteen 0xff, the indices 0 to 15, sixteen 0xff: read at `r` it moves the low `r` bytes
    /// of a register to its top, read at `r + 16` it moves the rest down and marks the top `r`
    /// bytes for the blend.
    shift: [u8; 48],
}

const KEYS: [u64; 23] = match crate::crc32::consts::CRC32_ISO_HDLC.keys {
    crate::CrcKeysStorage::KeysFold256(keys) => keys,
    crate::CrcKeysStorage::KeysFutureTest(_) => panic!("CRC-32/ISO-HDLC has 23 keys"),
};

#[cfg(not(miri))]
static CONSTS: Consts = Consts {
    k4: pair(4),
    k2: pair(2),
    k1: pair(1),
    fold96: [KEYS[5], 0],
    barrett: [KEYS[0], KEYS[8]],
    _pad: [0; 2],
    shift: {
        let mut t = [0xffu8; 48];
        let mut i = 0;
        while i < 16 {
            t[16 + i] = i as u8;
            i += 1;
        }
        t
    },
};

/// The pairs are the generated table's, swapped: keys 1 and 2 fold one block, 19 and 20 two, 15
/// and 16 four.
const _: () = {
    assert!(pair(1)[1] == KEYS[1] && pair(1)[0] == KEYS[2]);
    assert!(pair(2)[1] == KEYS[19] && pair(2)[0] == KEYS[20]);
    assert!(pair(4)[1] == KEYS[15] && pair(4)[0] == KEYS[16]);
    assert!(KEYS[0] != 0);
};

#[cfg(not(miri))]
const K4: usize = core::mem::offset_of!(Consts, k4);
#[cfg(not(miri))]
const K2: usize = core::mem::offset_of!(Consts, k2);
#[cfg(not(miri))]
const K1: usize = core::mem::offset_of!(Consts, k1);
#[cfg(not(miri))]
const FOLD96: usize = core::mem::offset_of!(Consts, fold96);
#[cfg(not(miri))]
const BARRETT: usize = core::mem::offset_of!(Consts, barrett);
#[cfg(not(miri))]
const SHIFT: usize = core::mem::offset_of!(Consts, shift);

/// The CRC-32/ISO-HDLC register after `bytes` from `state`, the register before them (not the
/// published CRC), for 16 bytes up to 1023, in line in the checksum entry: the 16 to 63 byte
/// block first, so those lengths take one taken branch fewer than in libdeflate's order.
///
/// # Safety
///
/// The CPU must have AVX, SSE4.1 and PCLMULQDQ, and `bytes` must be 16 to 1023 bytes long.
#[inline(always)]
unsafe fn update(state: u32, bytes: &[u8]) -> u32 {
    debug_assert!(bytes.len() >= 16 && bytes.len() < 1024);
    // Miri runs the same shape from intrinsics.
    #[cfg(miri)]
    {
        !(crate::arch::x86_64::iso_hdlc_xmm::iso_hdlc_xmm(!state, bytes) as u32)
    }
    #[cfg(not(miri))]
    {
        let out: u32;
        // SAFETY: every read is within `bytes`, whose length is 16 to 1023: whole blocks before
        // `{n}` and the last 16 bytes; the constants are read within `CONSTS`.
        asm!(
            "vmovd xmm0, {s:e}",
            "vpxor xmm0, xmm0, xmmword ptr [{p}]",
            "cmp {n}, 63",
            "ja 20f",
            // 16 to 63 bytes: one accumulator, block by block.
            "vmovdqa xmm6, xmmword ptr [rip + {c} + {k1}]",
            "lea {q}, [{p} + 16]",
            "cmp {n}, 31",
            "jbe 25f",
            "vpclmulqdq xmm1, xmm0, xmm6, 0x00",
            "vpclmulqdq xmm0, xmm0, xmm6, 0x11",
            "lea {q}, [{p} + 32]",
            "vpxor xmm0, xmm1, xmm0",
            "vpxor xmm0, xmm0, xmmword ptr [{p} + 16]",
            "cmp {n}, 47",
            "jbe 25f",
            "vpclmulqdq xmm1, xmm0, xmm6, 0x00",
            "vpclmulqdq xmm0, xmm0, xmm6, 0x11",
            "lea {q}, [{p} + 48]",
            "vpxor xmm0, xmm1, xmm0",
            "vpxor xmm0, xmm0, xmmword ptr [{p} + 32]",
            "jmp 25f",
            // 64 bytes and more: four accumulators, then onto one; falls into the tail.
            "20:",
            "vmovdqu xmm7, xmmword ptr [{p} + 16]",
            "vmovdqu xmm4, xmmword ptr [{p} + 32]",
            "lea {q}, [{p} + 64]",
            "vmovdqu xmm2, xmmword ptr [{p} + 48]",
            "cmp {n}, 127",
            "jbe 22f",
            "lea {t}, [{n} - 128]",
            "vmovdqa xmm6, xmmword ptr [rip + {c} + {k4}]",
            "shr {t}, 6",
            "lea {e}, [{t} + 2]",
            "shl {e}, 6",
            "add {e}, {p}",
            ".p2align 4",
            "21:",
            "vpclmulqdq xmm3, xmm7, xmm6, 0x11",
            "vpclmulqdq xmm1, xmm7, xmm6, 0x00",
            "vpclmulqdq xmm5, xmm0, xmm6, 0x00",
            "add {q}, 64",
            "vpxor xmm1, xmm1, xmm3",
            "vpclmulqdq xmm3, xmm4, xmm6, 0x00",
            "vpclmulqdq xmm4, xmm4, xmm6, 0x11",
            "vpxor xmm3, xmm3, xmm4",
            "vpclmulqdq xmm0, xmm0, xmm6, 0x11",
            "vpxor xmm4, xmm3, xmmword ptr [{q} - 32]",
            "vpclmulqdq xmm3, xmm2, xmm6, 0x00",
            "vpclmulqdq xmm2, xmm2, xmm6, 0x11",
            "vpxor xmm0, xmm0, xmm5",
            "vpxor xmm2, xmm2, xmm3",
            "vpxor xmm0, xmm0, xmmword ptr [{q} - 64]",
            "vpxor xmm7, xmm1, xmmword ptr [{q} - 48]",
            "vpxor xmm2, xmm2, xmmword ptr [{q} - 16]",
            "cmp {q}, {e}",
            "jne 21b",
            // Four accumulators onto two, then the next 32 bytes if the length has them.
            "22:",
            "vmovdqa xmm5, xmmword ptr [rip + {c} + {k2}]",
            "vpclmulqdq xmm3, xmm0, xmm5, 0x00",
            "vpclmulqdq xmm0, xmm0, xmm5, 0x11",
            "vpclmulqdq xmm1, xmm7, xmm5, 0x11",
            "vpxor xmm3, xmm3, xmm0",
            "vpclmulqdq xmm0, xmm7, xmm5, 0x00",
            "vpxor xmm1, xmm1, xmm0",
            "vpxor xmm3, xmm3, xmm4",
            "vpxor xmm1, xmm1, xmm2",
            "test {n:l}, 32",
            "je 24f",
            "vpclmulqdq xmm0, xmm3, xmm5, 0x00",
            "vpclmulqdq xmm3, xmm3, xmm5, 0x11",
            "add {q}, 32",
            "vpxor xmm0, xmm0, xmm3",
            "vpxor xmm3, xmm0, xmmword ptr [{q} - 32]",
            "vpclmulqdq xmm0, xmm1, xmm5, 0x00",
            "vpclmulqdq xmm1, xmm1, xmm5, 0x11",
            "vpxor xmm0, xmm0, xmm1",
            "vpxor xmm1, xmm0, xmmword ptr [{q} - 16]",
            // Two onto one, then the next 16 bytes if the length has them.
            "24:",
            "vmovdqa xmm6, xmmword ptr [rip + {c} + {k1}]",
            "vpclmulqdq xmm0, xmm3, xmm6, 0x00",
            "vpclmulqdq xmm3, xmm3, xmm6, 0x11",
            "vpxor xmm0, xmm0, xmm3",
            "vpxor xmm0, xmm0, xmm1",
            "test {n:l}, 16",
            "je 25f",
            "vpclmulqdq xmm1, xmm0, xmm6, 0x00",
            "vpclmulqdq xmm0, xmm0, xmm6, 0x11",
            "add {q}, 16",
            "vpxor xmm0, xmm1, xmm0",
            "vpxor xmm0, xmm0, xmmword ptr [{q} - 16]",
            // The bytes after the last whole block, read overlapping it.
            "25:",
            "mov {t}, {n}",
            "and {t:e}, 15",
            "je 26f",
            "lea {e}, [rip + {c} + {shift}]",
            "vmovdqu xmm2, xmmword ptr [{e} + {t} + 16]",
            "vpshufb xmm1, xmm0, xmmword ptr [{e} + {t}]",
            "vpshufb xmm0, xmm0, xmm2",
            "vpblendvb xmm0, xmm0, xmmword ptr [{q} + {t} - 16], xmm2",
            "vpclmulqdq xmm2, xmm1, xmm6, 0x00",
            "vpclmulqdq xmm1, xmm1, xmm6, 0x11",
            "vpxor xmm1, xmm2, xmm1",
            "vpxor xmm0, xmm1, xmm0",
            // 128 bits to 96 by x^96, then the 96-bit Barrett step; the register is the third
            // 32-bit word.
            "26:",
            "vpclmulqdq xmm1, xmm0, xmmword ptr [rip + {c} + {fold96}], 0x00",
            "vpsrldq xmm0, xmm0, 8",
            "vpxor xmm0, xmm0, xmm1",
            "vmovdqa xmm2, xmmword ptr [rip + {c} + {barrett}]",
            "vpclmulqdq xmm1, xmm0, xmm2, 0x00",
            "vpclmulqdq xmm1, xmm1, xmm2, 0x10",
            "vpxor xmm0, xmm0, xmm1",
            "vpextrd {s:e}, xmm0, 2",
            s = inout(reg) state => out,
            p = in(reg) bytes.as_ptr(),
            n = in(reg) bytes.len(),
            q = out(reg) _,
            t = out(reg) _,
            e = out(reg) _,
            c = sym CONSTS,
            k4 = const K4,
            k2 = const K2,
            k1 = const K1,
            fold96 = const FOLD96,
            barrett = const BARRETT,
            shift = const SHIFT,
            out("xmm0") _,
            out("xmm1") _,
            out("xmm2") _,
            out("xmm3") _,
            out("xmm4") _,
            out("xmm5") _,
            out("xmm6") _,
            out("xmm7") _,
            options(pure, readonly, nostack),
        );
        out
    }
}

/// The E-core parts' CRC-32/ISO-HDLC checksum entry: [`update`] from 16 bytes up to
/// [`ASM_END`], the SSE tier's 128-bit fold from there to `MID_END`, the tier's long fold after
/// that, and the short path below 16 bytes.
///
/// # Safety
///
/// The CPU must have AVX2, VPCLMULQDQ, SSE4.1 and PCLMULQDQ.
#[inline(never)]
#[target_feature(enable = "avx2,vpclmulqdq,sse4.1,pclmulqdq")]
pub(crate) unsafe fn iso_hdlc_checksum_ecore(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    if len.wrapping_sub(16) < ASM_END - 16 {
        return !update(!0, bytes) as u64;
    }
    if len >= ASM_END {
        if len >= crate::arch::MID_END {
            return crate::arch::iso_hdlc_fold_x86_64_avx2_vpclmulqdq(0, bytes);
        }
        return crate::arch::iso_hdlc_mid_x86_sse_avx(0, bytes);
    }
    short(bytes)
}

/// AMD family 0x17's (Zen 2's) CRC-32/ISO-HDLC checksum entry on the SSE tier with AVX: the
/// same [`update`] from 16 bytes up to [`ASM_END`], built with the features that tier has
/// (AVX, SSE4.1, PCLMULQDQ; [`update`] needs no more), then the SSE tier's 128-bit folds in the
/// AVX encoding, as [`crate::arch::iso_hdlc_checksum_x86_sse_avx`] takes them.
///
/// # Safety
///
/// The CPU must have AVX, SSE4.1 and PCLMULQDQ.
#[inline(never)]
#[target_feature(enable = "avx,sse4.1,pclmulqdq")]
pub(crate) unsafe fn iso_hdlc_checksum_zen2(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    if len.wrapping_sub(16) < ASM_END - 16 {
        return !update(!0, bytes) as u64;
    }
    if len >= ASM_END {
        if len >= crate::arch::MID_END {
            return crate::arch::iso_hdlc_fold_x86_sse_avx(0, bytes);
        }
        return crate::arch::iso_hdlc_mid_x86_sse_avx(0, bytes);
    }
    short_sse(bytes)
}

/// Below 16 bytes on the SSE tier, out of the entry's way.
#[cold]
#[inline(never)]
#[target_feature(enable = "avx,sse4.1,pclmulqdq")]
unsafe fn short_sse(bytes: &[u8]) -> u64 {
    !crate::algorithm::update_short::<_, crate::structs::Width32>(
        !0,
        bytes,
        &crate::crc32::consts::CRC32_ISO_HDLC,
        &crate::arch::x86::sse::X86SsePclmulqdqOps,
    ) as u64
}

/// Below 16 bytes, out of the entry's way.
#[cold]
#[inline(never)]
#[target_feature(enable = "avx2,vpclmulqdq,sse4.1,pclmulqdq")]
unsafe fn short(bytes: &[u8]) -> u64 {
    let ops = crate::arch::x86_64::avx2_vpclmulqdq::X86_64Avx2VpclmulqdqOps::new();
    !crate::algorithm::update_short::<_, crate::structs::Width32>(
        !0,
        bytes,
        &crate::crc32::consts::CRC32_ISO_HDLC,
        &ops,
    ) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every length from 16 to 1023, from four starts, from the initial register and a carried
    /// one, forced onto the assembly, against the software path; and the checksum entry over
    /// every length from 0 to 1100.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn ecore_asm_tier_matches_software() {
        if !(is_x86_feature_detected!("avx2")
            && is_x86_feature_detected!("vpclmulqdq")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1"))
        {
            eprintln!("skipping the E-core CRC-32/ISO-HDLC entry: the CPU lacks VPCLMULQDQ");
            return;
        }
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let buf: Vec<u8> = (0..1200u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        for len in 16..1024 {
            for off in [0usize, 1, 15, 33] {
                let data = &buf[off..off + len];
                for state in [!0u32, 0x1234_5678] {
                    let want = crate::arch::software::update(state as u64, data, params) as u32;
                    let got = unsafe { update(state, data) };
                    assert_eq!(got, want, "len {len} off {off} state {state:#x}");
                }
            }
        }
        for len in 0..=1100 {
            for off in [0usize, 7] {
                let data = &buf[off..off + len];
                let want = !(crate::arch::software::update(!0, data, params) as u32);
                let got = unsafe { iso_hdlc_checksum_ecore(data) } as u32;
                assert_eq!(got, want, "checksum len {len} off {off}");
            }
        }
    }

    /// Zen 2's checksum entry, forced onto it, against the software path over every length
    /// from 0 to 1100 from four starts, and across the hand-over to the long fold.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn zen2_entry_tier_matches_software() {
        if !(is_x86_feature_detected!("avx")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1"))
        {
            eprintln!("skipping Zen 2's CRC-32/ISO-HDLC entry: the CPU lacks AVX or PCLMULQDQ");
            return;
        }
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let buf: Vec<u8> = (0..4300u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let lens = (0..=1100).chain([4095, 4096, 4097, 4200]);
        for len in lens {
            for off in [0usize, 1, 15, 33] {
                let data = &buf[off..off + len];
                let want = !(crate::arch::software::update(!0, data, params) as u32);
                let got = unsafe { iso_hdlc_checksum_zen2(data) } as u32;
                assert_eq!(got, want, "checksum len {len} off {off}");
            }
        }
    }
}
