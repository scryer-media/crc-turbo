//! The lane kernels: four or eight 128-bit PMULL folds side by side, then one lane carried 16
//! bytes at a time, reduced by two CRC32 instructions; and the PMULL tier's large-buffer kernel,
//! four CRC32 instruction streams beside three of those folds.
//!
//! These are the shapes the fast-crc32 generator calls `v4`, `v8` and `v3s4x2e_v2`
//! (<https://github.com/corsix/fast-crc32/>, MIT), with a 16-byte tail added to the first two so
//! a remainder past the last full stride stays in the vector unit instead of running the CRC32
//! instruction chain. Between 64 bytes and 2 KiB the lane kernels ran 1.1 to 2.0 times the speed
//! of 1.11.0's kernels on Graviton 2, 3 and 4 and on an Apple M5 Max: a shorter stride starts
//! folding sooner, and fewer lanes take fewer steps to reduce.
//!
//! Each fold step has two forms. On the SHA3 tier the two products and the data are combined by
//! one EOR3, which Graviton 3 and 4 run faster. Elsewhere each product is combined with an EOR
//! of its own, which on Apple's cores is written as an adjacent pair the core fuses (see
//! [`clmul_lo_and_xor`]); Apple's cores take that form on the SHA3 tier too.
//!
//! The fold keys are `x^n mod P` for each distance, computed at compile time by
//! [`shifts::fold_keys`].

#![cfg(target_arch = "aarch64")]

use crate::crc32::fusion::aarch64::iscsi::crc_pmull_sha3::shift_iscsi;
use crate::crc32::fusion::aarch64::iso_hdlc::crc_pmull_sha3::shift_iso_hdlc;
use crate::crc32::fusion::aarch64::{
    clmul_hi_and_xor, clmul_lo_and_xor, clmul_scalar, crc32_iscsi_small_fast,
    crc32_iso_hdlc_small_fast, shifts,
};
use core::arch::aarch64::{
    __crc32b, __crc32cb, __crc32cd, __crc32d, uint64x2_t, veorq_u64, vgetq_lane_u64, vld1q_u64,
    vmovq_n_u64, vsetq_lane_u64,
};

#[cfg(not(target_vendor = "apple"))]
use crate::crc32::fusion::aarch64::veor3q_u64;
#[cfg(not(target_vendor = "apple"))]
use crate::crc32::fusion::aarch64::{clmul_hi, clmul_lo};

/// `x` folded forward over the 16 bytes at the distance `k` was made for, plus `data`.
#[inline]
#[target_feature(enable = "aes")]
unsafe fn fold_pair(x: uint64x2_t, k: uint64x2_t, data: uint64x2_t) -> uint64x2_t {
    let y = clmul_lo_and_xor(x, k, data);
    clmul_hi_and_xor(x, k, y)
}

/// [`fold_pair`] with each product and its EOR written as one adjacent pair in assembly, as
/// fast-crc32's `v3s4x2e_v2` writes them (its `clmul_lo_e` and `clmul_hi_e`), on every target:
/// the compiler's own schedule of [`fold_pair`] may move the EOR away from its product. The
/// stream kernels' fold.
#[cfg(not(miri))]
#[inline]
#[target_feature(enable = "aes")]
unsafe fn fold_pair_adjacent(x: uint64x2_t, k: uint64x2_t, data: uint64x2_t) -> uint64x2_t {
    let y: uint64x2_t;
    let r: uint64x2_t;
    // SAFETY: `pmull` and `pmull2` need the `aes` feature this function is compiled with; the
    // pairs touch registers only
    unsafe {
        core::arch::asm!(
            "pmull {y:v}.1q, {x:v}.1d, {k:v}.1d",
            "eor {y:v}.16b, {y:v}.16b, {d:v}.16b",
            y = out(vreg) y,
            x = in(vreg) x,
            k = in(vreg) k,
            d = in(vreg) data,
            options(pure, nomem, nostack, preserves_flags),
        );
        core::arch::asm!(
            "pmull2 {x:v}.1q, {x:v}.2d, {k:v}.2d",
            "eor {x:v}.16b, {x:v}.16b, {y:v}.16b",
            x = inout(vreg) x => r,
            k = in(vreg) k,
            y = in(vreg) y,
            options(pure, nomem, nostack, preserves_flags),
        );
    }
    r
}

/// Miri runs no assembly: there the pairs are [`fold_pair`]'s intrinsics, the same value.
#[cfg(miri)]
#[inline]
#[target_feature(enable = "aes")]
unsafe fn fold_pair_adjacent(x: uint64x2_t, k: uint64x2_t, data: uint64x2_t) -> uint64x2_t {
    fold_pair(x, k, data)
}

/// [`fold_pair`] with one EOR3 for the two products and the data.
#[cfg(not(target_vendor = "apple"))]
#[inline]
#[target_feature(enable = "aes,sha3")]
unsafe fn fold_eor3(x: uint64x2_t, k: uint64x2_t, data: uint64x2_t) -> uint64x2_t {
    veor3q_u64(clmul_lo(x, k), clmul_hi(x, k), data)
}

/// Apple's cores fuse the pair form and gain nothing from EOR3.
#[cfg(target_vendor = "apple")]
#[inline]
#[target_feature(enable = "aes,sha3")]
unsafe fn fold_eor3(x: uint64x2_t, k: uint64x2_t, data: uint64x2_t) -> uint64x2_t {
    fold_pair(x, k, data)
}

#[inline(always)]
unsafe fn load(p: *const u8) -> uint64x2_t {
    // SAFETY: the caller holds at least 16 readable bytes at `p`; NEON loads need no alignment
    unsafe { vld1q_u64(p as *const u64) }
}

/// The key pair `k`, loaded from memory as one vector.
///
/// Left to itself LLVM splits a constant pair into its halves, since `pmull` and `pmull2` each
/// read one, and builds each from immediates and a move from the general registers into the
/// vector unit. fast-crc32's C loads the pair with one `ldr`. Passing the pair through an empty
/// `asm!` keeps it one vector, which LLVM then loads.
///
/// Apple's cores keep LLVM's form: there the load cost the eight-lane kernel 12 to 16% at 256
/// and 512 bytes on an Apple M5 Max.
#[inline(always)]
unsafe fn load_key(k: &[u64; 2]) -> uint64x2_t {
    // SAFETY: `k` is 16 readable bytes; the template is empty and only hides the value
    unsafe {
        #[allow(unused_mut)]
        let mut v = vld1q_u64(k.as_ptr());
        #[cfg(not(any(target_vendor = "apple", miri)))]
        core::arch::asm!(
            "/* {0:v} */",
            inout(vreg) v,
            options(pure, nomem, nostack, preserves_flags)
        );
        v
    }
}

/// Whether the PMULL tier's lane kernels fold a remainder of 16 bytes or more in the vector
/// unit, as the SHA3 tier's do, rather than run it through the CRC32 instruction. On Graviton 2
/// the vector tail was no faster over the sizes it touches: 8.7 GB/s against 9.0 at 112 bytes,
/// though 9.5 against 8.7 at 80 bytes of CRC-32/ISO-HDLC, so the instruction stays there. Apple's
/// cores take the vector tail on both tiers.
const VECTOR_TAIL_PMULL: bool = cfg!(target_vendor = "apple");

macro_rules! lane_kernel {
    ($(#[$meta:meta])* $name:ident, $lanes:literal, $features:literal, $fold:ident, $keys:path,
     $crc_d:ident, $crc_b:ident, $tail16:expr) => {
        $(#[$meta])*
        ///
        /// # Safety
        ///
        /// `buf` holds `len` readable bytes, and the CPU has the CRC32 instructions and the
        /// features this function is compiled with.
        #[inline]
        #[target_feature(enable = $features)]
        pub(crate) unsafe fn $name(mut crc0: u32, mut buf: *const u8, mut len: usize) -> u32 {
            const LANES: usize = $lanes;
            const STRIDE: usize = 16 * LANES;
            const K_STRIDE: [u64; 2] = $keys(STRIDE);
            const K_16: [u64; 2] = $keys(16);
            // Pairs of lanes 16, 32 and 64 bytes apart.
            const K_REDUCE: [[u64; 2]; 3] = [$keys(16), $keys(32), $keys(64)];

            // Unlike the generator's kernels, these do not first align the buffer to 16 bytes:
            // NEON loads need no alignment, and below 2 KiB the prologue's branches cost more
            // than the unaligned loads they avoid. On an Apple M5 Max, dropping it took the
            // four-lane kernel from 18 to 33 GB/s at 64 bytes and the eight-lane one from 62 to
            // 79 at 256.
            if len >= STRIDE {
                let mut x = [vmovq_n_u64(0); LANES];
                for (i, lane) in x.iter_mut().enumerate() {
                    *lane = load(buf.add(16 * i));
                }
                x[0] = veorq_u64(x[0], vsetq_lane_u64::<0>(crc0 as u64, vmovq_n_u64(0)));
                buf = buf.add(STRIDE);
                len -= STRIDE;

                let k = load_key(&K_STRIDE);
                while len >= STRIDE {
                    for (i, lane) in x.iter_mut().enumerate() {
                        *lane = $fold(*lane, k, load(buf.add(16 * i)));
                    }
                    buf = buf.add(STRIDE);
                    len -= STRIDE;
                }

                // Fold the lanes into the first: neighbours, then pairs, then quads.
                let mut level = 0;
                let mut step = 1;
                while step < LANES {
                    let k = load_key(&K_REDUCE[level]);
                    let mut i = 0;
                    while i < LANES {
                        x[i] = $fold(x[i], k, x[i + step]);
                        i += 2 * step;
                    }
                    level += 1;
                    step *= 2;
                }

                if $tail16 {
                    let k = load_key(&K_16);
                    while len >= 16 {
                        x[0] = $fold(x[0], k, load(buf));
                        buf = buf.add(16);
                        len -= 16;
                    }
                }

                // Reduce 128 bits to 32, and multiply by x^32.
                crc0 = $crc_d(0, vgetq_lane_u64::<0>(x[0]));
                crc0 = $crc_d(crc0, vgetq_lane_u64::<1>(x[0]));
            }

            while len >= 8 {
                crc0 = $crc_d(crc0, (buf as *const u64).read_unaligned());
                buf = buf.add(8);
                len -= 8;
            }
            while len > 0 {
                crc0 = $crc_b(crc0, *buf);
                buf = buf.add(1);
                len -= 1;
            }

            crc0
        }
    };
}

lane_kernel!(
    /// CRC-32/ISCSI, four lanes.
    crc32_iscsi_v4, 4, "crc,aes", fold_pair, shifts::iscsi_fold_keys,
    __crc32cd, __crc32cb, VECTOR_TAIL_PMULL
);
lane_kernel!(
    /// CRC-32/ISCSI, eight lanes.
    crc32_iscsi_v8, 8, "crc,aes", fold_pair, shifts::iscsi_fold_keys,
    __crc32cd, __crc32cb, VECTOR_TAIL_PMULL
);
lane_kernel!(
    /// CRC-32/ISCSI, four lanes, EOR3 folds. Linux and Android run the
    /// assembly copy in `asm`, which fixes where the kernel sits.
    #[cfg_attr(
        all(target_endian = "little", not(miri), any(target_os = "linux", target_os = "android")),
        allow(dead_code)
    )]
    crc32_iscsi_eor3_v4, 4, "crc,aes,sha3", fold_eor3, shifts::iscsi_fold_keys, __crc32cd,
    __crc32cb, true
);
lane_kernel!(
    /// CRC-32/ISCSI, eight lanes, EOR3 folds.
    crc32_iscsi_eor3_v8, 8, "crc,aes,sha3", fold_eor3, shifts::iscsi_fold_keys, __crc32cd,
    __crc32cb, true
);
lane_kernel!(
    /// CRC-32/ISO-HDLC, four lanes.
    crc32_iso_hdlc_v4, 4, "crc,aes", fold_pair, shifts::iso_hdlc_fold_keys,
    __crc32d, __crc32b, VECTOR_TAIL_PMULL
);
lane_kernel!(
    /// CRC-32/ISO-HDLC, eight lanes.
    crc32_iso_hdlc_v8, 8, "crc,aes", fold_pair, shifts::iso_hdlc_fold_keys,
    __crc32d, __crc32b, VECTOR_TAIL_PMULL
);
lane_kernel!(
    /// CRC-32/ISO-HDLC, four lanes, EOR3 folds. Linux and Android run the
    /// assembly copy in `asm`, which fixes where the kernel sits.
    #[cfg_attr(
        all(target_endian = "little", not(miri), any(target_os = "linux", target_os = "android")),
        allow(dead_code)
    )]
    crc32_iso_hdlc_eor3_v4, 4, "crc,aes,sha3", fold_eor3, shifts::iso_hdlc_fold_keys, __crc32d,
    __crc32b, true
);
lane_kernel!(
    /// CRC-32/ISO-HDLC, eight lanes, EOR3 folds.
    crc32_iso_hdlc_eor3_v8, 8, "crc,aes,sha3", fold_eor3, shifts::iso_hdlc_fold_keys, __crc32d,
    __crc32b, true
);

/// Four CRC32 instruction streams beside a three-lane fold, the generator's `v3s4x2e_v2`.
///
/// The buffer is taken in blocks of 112 bytes: each stream runs over a run of 16 bytes per
/// block, one after another, and the fold over the 48 bytes per block that follow all four.
/// Each stream's CRC is then moved past the bytes after it by the shift tables' multiplier and
/// added to the fold's. On Graviton 2, which has no SHA3, it ran 1.5 to 1.6 times the speed of
/// 1.11.0's twelve-lane `v12e_v1` fold from 2 KiB to 1 MiB: the core issues the CRC32
/// instructions and the folds side by side. What is left after the last block, under 112
/// bytes, runs the CRC32 instruction loop, which on Graviton 2 outran the four-lane fold at
/// those lengths; Apple's cores, where that was not measured, run the four-lane fold.
macro_rules! stream_kernel {
    ($(#[$meta:meta])* $name:ident, $keys:path, $shift:path, $crc_d:ident, $crc_b:ident,
     $tail:ident) => {
        $(#[$meta])*
        ///
        /// # Safety
        ///
        /// `buf` holds `len` readable bytes, and the CPU has the CRC32 instructions and PMULL.
        #[inline]
        #[target_feature(enable = "crc,aes")]
        pub(crate) unsafe fn $name(mut crc0: u32, mut buf: *const u8, mut len: usize) -> u32 {
            const BLOCK: usize = 112;
            const K_48: [u64; 2] = $keys(48);
            const K_16: [u64; 2] = $keys(16);

            while len > 0 && (buf as usize & 7) != 0 {
                crc0 = $crc_b(crc0, *buf);
                buf = buf.add(1);
                len -= 1;
            }
            if (buf as usize & 8) != 0 && len >= 8 {
                crc0 = $crc_d(crc0, (buf as *const u64).read());
                buf = buf.add(8);
                len -= 8;
            }

            if len >= BLOCK {
                let blk = len / BLOCK;
                let klen = blk * 16;
                let end = buf.add(len);
                let mut buf2 = buf.add(4 * klen);
                let (mut crc1, mut crc2, mut crc3) = (0u32, 0u32, 0u32);

                let mut x0 = load(buf2);
                let mut x1 = load(buf2.add(16));
                let mut x2 = load(buf2.add(32));
                buf2 = buf2.add(48);

                // Both pointers are 16-byte aligned here, and `klen` is a multiple of 16.
                let word = |p: *const u8| (p as *const u64).read();
                let k = load_key(&K_48);
                for _ in 1..blk {
                    x0 = fold_pair_adjacent(x0, k, load(buf2));
                    x1 = fold_pair_adjacent(x1, k, load(buf2.add(16)));
                    x2 = fold_pair_adjacent(x2, k, load(buf2.add(32)));
                    crc0 = $crc_d(crc0, word(buf));
                    crc1 = $crc_d(crc1, word(buf.add(klen)));
                    crc2 = $crc_d(crc2, word(buf.add(2 * klen)));
                    crc3 = $crc_d(crc3, word(buf.add(3 * klen)));
                    crc0 = $crc_d(crc0, word(buf.add(8)));
                    crc1 = $crc_d(crc1, word(buf.add(klen + 8)));
                    crc2 = $crc_d(crc2, word(buf.add(2 * klen + 8)));
                    crc3 = $crc_d(crc3, word(buf.add(3 * klen + 8)));
                    buf = buf.add(16);
                    buf2 = buf2.add(48);
                }

                let k = load_key(&K_16);
                x0 = fold_pair_adjacent(x0, k, x1);
                x0 = fold_pair_adjacent(x0, k, x2);

                crc0 = $crc_d(crc0, word(buf));
                crc1 = $crc_d(crc1, word(buf.add(klen)));
                crc2 = $crc_d(crc2, word(buf.add(2 * klen)));
                crc3 = $crc_d(crc3, word(buf.add(3 * klen)));
                crc0 = $crc_d(crc0, word(buf.add(8)));
                crc1 = $crc_d(crc1, word(buf.add(klen + 8)));
                crc2 = $crc_d(crc2, word(buf.add(2 * klen + 8)));
                crc3 = $crc_d(crc3, word(buf.add(3 * klen + 8)));

                let vector = blk * 48;
                let vc0 = clmul_scalar(crc0, $shift(3 * klen + vector));
                let vc1 = clmul_scalar(crc1, $shift(2 * klen + vector));
                let vc2 = clmul_scalar(crc2, $shift(klen + vector));
                let vc3 = clmul_scalar(crc3, $shift(vector));
                let vc = vgetq_lane_u64::<0>(veorq_u64(veorq_u64(vc0, vc1), veorq_u64(vc2, vc3)));

                // Reduce 128 bits to 32, and multiply by x^32.
                crc0 = $crc_d(0, vgetq_lane_u64::<0>(x0));
                crc0 = $crc_d(crc0, vc ^ vgetq_lane_u64::<1>(x0));

                buf = buf2;
                len = end.offset_from(buf) as usize;
            }

            $tail(crc0, buf, len)
        }
    };
}

/// The stream kernels' tail: the CRC32 instruction loop, or on Apple's cores the four-lane fold.
macro_rules! stream_tail {
    ($name:ident, $lanes:ident, $small:ident) => {
        #[inline(always)]
        unsafe fn $name(crc: u32, buf: *const u8, len: usize) -> u32 {
            // SAFETY: the stream kernel's caller holds `len` readable bytes at `buf` and runs on
            // a CPU with the CRC32 instructions and PMULL
            unsafe {
                if cfg!(target_vendor = "apple") {
                    $lanes(crc, buf, len)
                } else {
                    $small(crc, core::slice::from_raw_parts(buf, len))
                }
            }
        }
    };
}

stream_tail!(iscsi_stream_tail, crc32_iscsi_v4, crc32_iscsi_small_fast);
stream_tail!(
    iso_hdlc_stream_tail,
    crc32_iso_hdlc_v4,
    crc32_iso_hdlc_small_fast
);

stream_kernel!(
    /// CRC-32/ISCSI, four CRC32 streams and three lanes.
    crc32_iscsi_v3s4, shifts::iscsi_fold_keys, shift_iscsi, __crc32cd, __crc32cb,
    iscsi_stream_tail
);
stream_kernel!(
    /// CRC-32/ISO-HDLC, four CRC32 streams and three lanes.
    crc32_iso_hdlc_v3s4, shifts::iso_hdlc_fold_keys, shift_iso_hdlc, __crc32d, __crc32b,
    iso_hdlc_stream_tail
);
