//! The CRC32 instruction loops for short buffers, the four-lane fold for 96 to 255 bytes, and
//! Neoverse V2's four- and eight-lane folds (fast-crc32's `neon_eor3_*_v4` and `*_v8` shapes,
//! finished by the CRC32 instruction), written in assembly so that what runs, and where it sits, is fixed by this crate rather than
//! by the compiler that builds it.
//!
//! Compiled from Rust, the short loop's code was the same instruction for instruction as
//! 2ee9ff1's, yet CRC-32/ISO-HDLC from 16 to 48 bytes ran 0.90x to 0.93x of 2ee9ff1's speed on
//! Graviton 3 and 4: one cycle more a call. Aligning every function to 64 bytes in a build of
//! each moved those cells, some up and some down, so the cycle was where the code sat, not
//! what it was. Each function here is one `asm!` block that opens with `.p2align 6`: with
//! nothing compiled ahead of it, the directive raises the alignment of the function's own
//! section, so the entry lands on a 64-byte boundary with no padding executed. The lengths a
//! caller hashes most, 16, 32 and 48 bytes, take at most one branch in the short loop: the
//! length's bits pick the 32- and 16-byte steps. The Linux kernel's arm64 CRC32 routines pick
//! their tail steps by the length's bits the same way; no code is taken from them.
//!
//! Each loop is a non-generic `#[inline(never)]` function with the C calling convention, so its
//! arguments arrive in the registers the assembly names and the compiler adds only the return.
//! ELF targets only (Linux and Android), little-endian: Apple's cores keep the compiled loops,
//! which these were not measured against. The compiled loops in the parent module stay as the
//! other targets' path and as the reference the tests check these against.

#![cfg(all(
    target_arch = "aarch64",
    target_endian = "little",
    not(miri),
    any(target_os = "linux", target_os = "android")
))]

use super::shifts::{iscsi_fold_keys, iso_hdlc_fold_keys};

/// The 32-, 16-, 8-, 4-, 2- and 1-byte steps, each taken when its bit of the length in `x2` is
/// set, folding the bytes at `x1` into the state in `w0`. `$x`, `$w`, `$h` and `$b` are the
/// CRC32 instructions for 8, 4, 2 and 1 bytes. Ends at label `9`.
macro_rules! tail_steps {
    ($x:literal, $w:literal, $h:literal, $b:literal) => {
        concat!(
            // 32 bytes
            "tbz x2, #5, 2f\n",
            "ldp x4, x5, [x1]\n",
            "ldp x6, x7, [x1, #16]\n",
            "add x1, x1, #32\n",
            $x,
            " w0, w0, x4\n",
            $x,
            " w0, w0, x5\n",
            $x,
            " w0, w0, x6\n",
            $x,
            " w0, w0, x7\n",
            // 16 bytes
            "2:\n",
            "tbz x2, #4, 3f\n",
            "ldp x4, x5, [x1], #16\n",
            $x,
            " w0, w0, x4\n",
            $x,
            " w0, w0, x5\n",
            // A multiple of 16 is done
            "3:\n",
            "tst x2, #15\n",
            "b.eq 9f\n",
            // 8, 4, 2 and 1 bytes
            "tbz x2, #3, 4f\n",
            "ldr x4, [x1], #8\n",
            $x,
            " w0, w0, x4\n",
            "4:\n",
            "tbz x2, #2, 5f\n",
            "ldr w4, [x1], #4\n",
            $w,
            " w0, w0, w4\n",
            "5:\n",
            "tbz x2, #1, 6f\n",
            "ldrh w4, [x1], #2\n",
            $h,
            " w0, w0, w4\n",
            "6:\n",
            "tbz x2, #0, 9f\n",
            "ldrb w4, [x1]\n",
            $b,
            " w0, w0, w4\n",
            "9:\n",
        )
    };
}

/// The loop for a buffer shorter than 64 bytes: the tail steps alone.
macro_rules! short_loop {
    ($x:literal, $w:literal, $h:literal, $b:literal) => {
        concat!(".p2align 6\n", tail_steps!($x, $w, $h, $b))
    };
}

/// The loop for a buffer of any length: 64 bytes a turn, then the tail steps.
macro_rules! block_loop {
    ($x:literal, $w:literal, $h:literal, $b:literal) => {
        concat!(
            ".p2align 6\n",
            "cmp x2, #64\n",
            "b.lo 8f\n",
            "7:\n",
            "ldp x4, x5, [x1]\n",
            "ldp x6, x7, [x1, #16]\n",
            "ldp x8, x9, [x1, #32]\n",
            "ldp x10, x11, [x1, #48]\n",
            "add x1, x1, #64\n",
            "sub x2, x2, #64\n",
            $x,
            " w0, w0, x4\n",
            $x,
            " w0, w0, x5\n",
            $x,
            " w0, w0, x6\n",
            $x,
            " w0, w0, x7\n",
            $x,
            " w0, w0, x8\n",
            $x,
            " w0, w0, x9\n",
            $x,
            " w0, w0, x10\n",
            $x,
            " w0, w0, x11\n",
            "cmp x2, #64\n",
            "b.hs 7b\n",
            "8:\n",
            tail_steps!($x, $w, $h, $b)
        )
    };
}

/// The four-lane fold, EOR3 form, for 64 bytes and more: the state in `w0`, the buffer in
/// `x1`, its length in `x2`, and at `{keys}` the key pairs for 64, 16 and 32 bytes. `$x`, `$w`,
/// `$h` and `$b` are the CRC32 instructions.
macro_rules! v4_loop {
    ($x:literal, $w:literal, $h:literal, $b:literal) => {
        concat!(
            ".p2align 6\n",
            "adrp x6, {keys}\n",
            "add x6, x6, :lo12:{keys}\n",
            // The first 64 bytes, the state added to the first lane
            "ld1 {{v0.16b, v1.16b, v2.16b, v3.16b}}, [x1], #64\n",
            "fmov s4, w0\n",
            "ldr q5, [x6]\n",
            "sub x2, x2, #64\n",
            "eor v0.16b, v0.16b, v4.16b\n",
            "cmp x2, #64\n",
            "b.lo 3f\n",
            // Each further 64 bytes
            "2:\n",
            "ld1 {{v16.16b, v17.16b, v18.16b, v19.16b}}, [x1], #64\n",
            "sub x2, x2, #64\n",
            "pmull v20.1q, v0.1d, v5.1d\n",
            "pmull2 v21.1q, v0.2d, v5.2d\n",
            "pmull v22.1q, v1.1d, v5.1d\n",
            "pmull2 v23.1q, v1.2d, v5.2d\n",
            "pmull v24.1q, v2.1d, v5.1d\n",
            "pmull2 v25.1q, v2.2d, v5.2d\n",
            "pmull v26.1q, v3.1d, v5.1d\n",
            "pmull2 v27.1q, v3.2d, v5.2d\n",
            "eor3 v0.16b, v20.16b, v21.16b, v16.16b\n",
            "eor3 v1.16b, v22.16b, v23.16b, v17.16b\n",
            "eor3 v2.16b, v24.16b, v25.16b, v18.16b\n",
            "eor3 v3.16b, v26.16b, v27.16b, v19.16b\n",
            "cmp x2, #64\n",
            "b.hs 2b\n",
            // The lanes into the first: neighbours 16 bytes apart, then pairs 32 apart
            "3:\n",
            "ldr q5, [x6, #16]\n",
            "ldr q6, [x6, #32]\n",
            "pmull v20.1q, v0.1d, v5.1d\n",
            "pmull2 v21.1q, v0.2d, v5.2d\n",
            "pmull v22.1q, v2.1d, v5.1d\n",
            "pmull2 v23.1q, v2.2d, v5.2d\n",
            "eor3 v0.16b, v20.16b, v21.16b, v1.16b\n",
            "eor3 v2.16b, v22.16b, v23.16b, v3.16b\n",
            "pmull v20.1q, v0.1d, v6.1d\n",
            "pmull2 v21.1q, v0.2d, v6.2d\n",
            "eor3 v0.16b, v20.16b, v21.16b, v2.16b\n",
            // Each further 16 bytes
            "cmp x2, #16\n",
            "b.lo 5f\n",
            "4:\n",
            "ldr q16, [x1], #16\n",
            "sub x2, x2, #16\n",
            "pmull v20.1q, v0.1d, v5.1d\n",
            "pmull2 v21.1q, v0.2d, v5.2d\n",
            "eor3 v0.16b, v20.16b, v21.16b, v16.16b\n",
            "cmp x2, #16\n",
            "b.hs 4b\n",
            // 128 bits to 32, times x^32, then the last 15 bytes or fewer
            "5:\n",
            "fmov x4, d0\n",
            "mov x5, v0.d[1]\n",
            $x,
            " w0, wzr, x4\n",
            $x,
            " w0, w0, x5\n",
            "tbz x2, #3, 6f\n",
            "ldr x4, [x1], #8\n",
            $x,
            " w0, w0, x4\n",
            "6:\n",
            "tbz x2, #2, 7f\n",
            "ldr w4, [x1], #4\n",
            $w,
            " w0, w0, w4\n",
            "7:\n",
            "tbz x2, #1, 8f\n",
            "ldrh w4, [x1], #2\n",
            $h,
            " w0, w0, w4\n",
            "8:\n",
            "tbz x2, #0, 9f\n",
            "ldrb w4, [x1]\n",
            $b,
            " w0, w0, w4\n",
            "9:\n",
        )
    };
}

/// The lanes reduced to 128 bits in `v0`, that to 32 bits times `x^32` by two CRC32
/// instructions, then the remaining bytes, fewer than 128, by the CRC32 instruction alone: the
/// 64-byte step when bit 6 of `x2` is set, then [`tail_steps`]. This is how fast-crc32's `v4` and
/// `v8` kernels finish, with no 16-byte PMULL fold after the lanes.
macro_rules! crc_finish {
    ($x:literal, $w:literal, $h:literal, $b:literal) => {
        concat!(
            "fmov x4, d0\n",
            "mov x5, v0.d[1]\n",
            $x,
            " w0, wzr, x4\n",
            $x,
            " w0, w0, x5\n",
            // 64 bytes
            "tbz x2, #6, 1f\n",
            "ldp x4, x5, [x1]\n",
            "ldp x6, x7, [x1, #16]\n",
            $x,
            " w0, w0, x4\n",
            $x,
            " w0, w0, x5\n",
            $x,
            " w0, w0, x6\n",
            $x,
            " w0, w0, x7\n",
            "ldp x4, x5, [x1, #32]\n",
            "ldp x6, x7, [x1, #48]\n",
            "add x1, x1, #64\n",
            $x,
            " w0, w0, x4\n",
            $x,
            " w0, w0, x5\n",
            $x,
            " w0, w0, x6\n",
            $x,
            " w0, w0, x7\n",
            "1:\n",
            tail_steps!($x, $w, $h, $b)
        )
    };
}

/// Neoverse V2's four-lane fold, for 64 bytes and more: fast-crc32's `neon_eor3_*_v4`. The
/// stride loop and the lane reduction are [`v4_loop`]'s; the bytes past the last 64-byte
/// stride go through the CRC32 instruction ([`crc_finish`]) rather than a 16-byte PMULL fold.
/// Keys at `{keys}` as for [`v4_loop`].
macro_rules! v4_crc_tail_loop {
    ($x:literal, $w:literal, $h:literal, $b:literal) => {
        concat!(
            ".p2align 6\n",
            "adrp x6, {keys}\n",
            "add x6, x6, :lo12:{keys}\n",
            "ldp q0, q1, [x1]\n",
            "ldp q2, q3, [x1, #32]\n",
            "add x1, x1, #64\n",
            "fmov s4, w0\n",
            "ldr q5, [x6]\n",
            "sub x2, x2, #64\n",
            "eor v0.16b, v0.16b, v4.16b\n",
            "cmp x2, #64\n",
            "b.lo 13f\n",
            "12:\n",
            "ldp q16, q17, [x1]\n",
            "ldp q18, q19, [x1, #32]\n",
            "add x1, x1, #64\n",
            "sub x2, x2, #64\n",
            "pmull v20.1q, v0.1d, v5.1d\n",
            "pmull2 v0.1q, v0.2d, v5.2d\n",
            "eor3 v0.16b, v0.16b, v20.16b, v16.16b\n",
            "pmull v21.1q, v1.1d, v5.1d\n",
            "pmull2 v1.1q, v1.2d, v5.2d\n",
            "eor3 v1.16b, v1.16b, v21.16b, v17.16b\n",
            "pmull v22.1q, v2.1d, v5.1d\n",
            "pmull2 v2.1q, v2.2d, v5.2d\n",
            "eor3 v2.16b, v2.16b, v22.16b, v18.16b\n",
            "pmull v23.1q, v3.1d, v5.1d\n",
            "pmull2 v3.1q, v3.2d, v5.2d\n",
            "eor3 v3.16b, v3.16b, v23.16b, v19.16b\n",
            "cmp x2, #64\n",
            "b.hs 12b\n",
            // Lanes 16 bytes apart, then the pair 32 apart
            "13:\n",
            "ldr q5, [x6, #16]\n",
            "ldr q6, [x6, #32]\n",
            "pmull v20.1q, v0.1d, v5.1d\n",
            "pmull2 v0.1q, v0.2d, v5.2d\n",
            "pmull v22.1q, v2.1d, v5.1d\n",
            "pmull2 v2.1q, v2.2d, v5.2d\n",
            "eor3 v0.16b, v0.16b, v20.16b, v1.16b\n",
            "eor3 v2.16b, v2.16b, v22.16b, v3.16b\n",
            "pmull v20.1q, v0.1d, v6.1d\n",
            "pmull2 v0.1q, v0.2d, v6.2d\n",
            "eor3 v0.16b, v0.16b, v20.16b, v2.16b\n",
            crc_finish!($x, $w, $h, $b)
        )
    };
}

/// Neoverse V2's eight-lane fold, for 128 bytes and more: fast-crc32's `neon_eor3_*_v8`, eight
/// lanes 128 bytes a stride with one EOR3 a lane, reduced in pairs 16, 32 and 64 bytes apart,
/// then [`crc_finish`]. At `{keys}` the key pairs for 128, 16, 32 and 64 bytes. Uses no
/// register in `v8` to `v15`, whose low halves a callee keeps.
macro_rules! v8_crc_tail_loop {
    ($x:literal, $w:literal, $h:literal, $b:literal) => {
        concat!(
            ".p2align 6\n",
            "adrp x6, {keys}\n",
            "add x6, x6, :lo12:{keys}\n",
            "ldp q0, q1, [x1]\n",
            "ldp q2, q3, [x1, #32]\n",
            "ldp q4, q5, [x1, #64]\n",
            "ldp q6, q7, [x1, #96]\n",
            "add x1, x1, #128\n",
            "fmov s24, w0\n",
            "ldr q31, [x6]\n",
            "sub x2, x2, #128\n",
            "eor v0.16b, v0.16b, v24.16b\n",
            "cmp x2, #128\n",
            "b.lo 13f\n",
            "12:\n",
            "ldp q16, q17, [x1]\n",
            "ldp q18, q19, [x1, #32]\n",
            "ldp q20, q21, [x1, #64]\n",
            "ldp q22, q23, [x1, #96]\n",
            "add x1, x1, #128\n",
            "sub x2, x2, #128\n",
            "pmull v24.1q, v0.1d, v31.1d\n",
            "pmull2 v0.1q, v0.2d, v31.2d\n",
            "eor3 v0.16b, v0.16b, v24.16b, v16.16b\n",
            "pmull v25.1q, v1.1d, v31.1d\n",
            "pmull2 v1.1q, v1.2d, v31.2d\n",
            "eor3 v1.16b, v1.16b, v25.16b, v17.16b\n",
            "pmull v26.1q, v2.1d, v31.1d\n",
            "pmull2 v2.1q, v2.2d, v31.2d\n",
            "eor3 v2.16b, v2.16b, v26.16b, v18.16b\n",
            "pmull v27.1q, v3.1d, v31.1d\n",
            "pmull2 v3.1q, v3.2d, v31.2d\n",
            "eor3 v3.16b, v3.16b, v27.16b, v19.16b\n",
            "pmull v24.1q, v4.1d, v31.1d\n",
            "pmull2 v4.1q, v4.2d, v31.2d\n",
            "eor3 v4.16b, v4.16b, v24.16b, v20.16b\n",
            "pmull v25.1q, v5.1d, v31.1d\n",
            "pmull2 v5.1q, v5.2d, v31.2d\n",
            "eor3 v5.16b, v5.16b, v25.16b, v21.16b\n",
            "pmull v26.1q, v6.1d, v31.1d\n",
            "pmull2 v6.1q, v6.2d, v31.2d\n",
            "eor3 v6.16b, v6.16b, v26.16b, v22.16b\n",
            "pmull v27.1q, v7.1d, v31.1d\n",
            "pmull2 v7.1q, v7.2d, v31.2d\n",
            "eor3 v7.16b, v7.16b, v27.16b, v23.16b\n",
            "cmp x2, #128\n",
            "b.hs 12b\n",
            // Pairs 16 bytes apart, then 32, then 64
            "13:\n",
            "ldr q28, [x6, #16]\n",
            "ldr q29, [x6, #32]\n",
            "ldr q30, [x6, #48]\n",
            "pmull v24.1q, v0.1d, v28.1d\n",
            "pmull2 v0.1q, v0.2d, v28.2d\n",
            "eor3 v0.16b, v0.16b, v24.16b, v1.16b\n",
            "pmull v25.1q, v2.1d, v28.1d\n",
            "pmull2 v2.1q, v2.2d, v28.2d\n",
            "eor3 v2.16b, v2.16b, v25.16b, v3.16b\n",
            "pmull v26.1q, v4.1d, v28.1d\n",
            "pmull2 v4.1q, v4.2d, v28.2d\n",
            "eor3 v4.16b, v4.16b, v26.16b, v5.16b\n",
            "pmull v27.1q, v6.1d, v28.1d\n",
            "pmull2 v6.1q, v6.2d, v28.2d\n",
            "eor3 v6.16b, v6.16b, v27.16b, v7.16b\n",
            "pmull v24.1q, v0.1d, v29.1d\n",
            "pmull2 v0.1q, v0.2d, v29.2d\n",
            "eor3 v0.16b, v0.16b, v24.16b, v2.16b\n",
            "pmull v25.1q, v4.1d, v29.1d\n",
            "pmull2 v4.1q, v4.2d, v29.2d\n",
            "eor3 v4.16b, v4.16b, v25.16b, v6.16b\n",
            "pmull v24.1q, v0.1d, v30.1d\n",
            "pmull2 v0.1q, v0.2d, v30.2d\n",
            "eor3 v0.16b, v0.16b, v24.16b, v4.16b\n",
            crc_finish!($x, $w, $h, $b)
        )
    };
}

/// Defines `$name`, an `#[inline(never)]` C-convention function running `$template` with the
/// state in `w0`, the buffer in `x1` and its length in `x2`, under `$features`, then adding the
/// final XOR in `w3` to the result; `$clobbers` are
/// the further registers the template writes, and `$($op)*` any further operands.
macro_rules! asm_fn {
    (
        $(#[$doc:meta])*
        $name:ident, $features:literal, $template:expr, [$($clobber:tt)*] $(, $($op:tt)*)?
    ) => {
        $(#[$doc])*
        ///
        /// # Safety
        ///
        /// `data` holds `len` readable bytes, and the CPU has the features named in the
        /// function's `target_feature`.
        #[inline(never)]
        #[target_feature(enable = $features)]
        pub(super) unsafe extern "C" fn $name(
            crc: u32,
            data: *const u8,
            len: usize,
            xorout: u32,
        ) -> u32 {
            let out: u32;
            // SAFETY: the caller holds `len` bytes at `data` and checked the features; the
            // template reads those bytes (and the keys it names), and writes only the registers
            // listed here
            unsafe {
                core::arch::asm!(
                    $template,
                    // The checksum's final XOR, so that the caller's call is its last act
                    "eor w0, w0, w3",
                    inout("w0") crc => out,
                    inout("x1") data => _,
                    inout("x2") len => _,
                    in("w3") xorout,
                    $($clobber)*
                    $($($op)*,)?
                    options(nostack, readonly, pure),
                );
            }
            out
        }
    };
}

asm_fn!(
    /// CRC-32/ISCSI of a buffer shorter than 64 bytes.
    crc32c_short,
    "crc",
    short_loop!("crc32cx", "crc32cw", "crc32ch", "crc32cb"),
    [out("x4") _, out("x5") _, out("x6") _, out("x7") _,]
);

asm_fn!(
    /// CRC-32/ISO-HDLC of a buffer shorter than 64 bytes.
    crc32_short,
    "crc",
    short_loop!("crc32x", "crc32w", "crc32h", "crc32b"),
    [out("x4") _, out("x5") _, out("x6") _, out("x7") _,]
);

asm_fn!(
    /// CRC-32/ISCSI of a buffer of any length by the CRC32 instruction alone.
    crc32c_blocks,
    "crc",
    block_loop!("crc32cx", "crc32cw", "crc32ch", "crc32cb"),
    [
        out("x4") _, out("x5") _, out("x6") _, out("x7") _, out("x8") _, out("x9") _,
        out("x10") _, out("x11") _,
    ]
);

asm_fn!(
    /// CRC-32/ISO-HDLC of a buffer of any length by the CRC32 instruction alone.
    crc32_blocks,
    "crc",
    block_loop!("crc32x", "crc32w", "crc32h", "crc32b"),
    [
        out("x4") _, out("x5") _, out("x6") _, out("x7") _, out("x8") _, out("x9") _,
        out("x10") _, out("x11") _,
    ]
);

/// The key pairs the CRC-32/ISCSI four-lane fold reads: 64, 16 and 32 bytes.
static ISCSI_V4_KEYS: [[u64; 2]; 3] = [
    iscsi_fold_keys(64),
    iscsi_fold_keys(16),
    iscsi_fold_keys(32),
];

/// The key pairs the CRC-32/ISO-HDLC four-lane fold reads: 64, 16 and 32 bytes.
static ISO_HDLC_V4_KEYS: [[u64; 2]; 3] = [
    iso_hdlc_fold_keys(64),
    iso_hdlc_fold_keys(16),
    iso_hdlc_fold_keys(32),
];

asm_fn!(
    /// CRC-32/ISCSI by the four-lane fold, EOR3 form, as [`super::lanes`]'s
    /// `crc32_iscsi_eor3_v4`, for 64 bytes and more.
    crc32c_eor3_v4,
    "crc,aes,sha3",
    v4_loop!("crc32cx", "crc32cw", "crc32ch", "crc32cb"),
    [
        out("x4") _, out("x5") _, out("x6") _,
        out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
        out("v6") _, out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
        out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _, out("v26") _,
        out("v27") _,
    ],
    keys = sym ISCSI_V4_KEYS
);

asm_fn!(
    /// [`crc32c_eor3_v4`] for CRC-32/ISO-HDLC.
    crc32_eor3_v4,
    "crc,aes,sha3",
    v4_loop!("crc32x", "crc32w", "crc32h", "crc32b"),
    [
        out("x4") _, out("x5") _, out("x6") _,
        out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
        out("v6") _, out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
        out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _, out("v26") _,
        out("v27") _,
    ],
    keys = sym ISO_HDLC_V4_KEYS
);

/// The key pairs the CRC-32/ISCSI eight-lane fold reads: 128, 16, 32 and 64 bytes.
static ISCSI_V8_KEYS: [[u64; 2]; 4] = [
    iscsi_fold_keys(128),
    iscsi_fold_keys(16),
    iscsi_fold_keys(32),
    iscsi_fold_keys(64),
];

/// The key pairs the CRC-32/ISO-HDLC eight-lane fold reads: 128, 16, 32 and 64 bytes.
static ISO_HDLC_V8_KEYS: [[u64; 2]; 4] = [
    iso_hdlc_fold_keys(128),
    iso_hdlc_fold_keys(16),
    iso_hdlc_fold_keys(32),
    iso_hdlc_fold_keys(64),
];

asm_fn!(
    /// CRC-32/ISCSI by Neoverse V2's four-lane fold ([`v4_crc_tail_loop`]), for 64 bytes and
    /// more.
    crc32c_v2_v4,
    "crc,aes,sha3",
    v4_crc_tail_loop!("crc32cx", "crc32cw", "crc32ch", "crc32cb"),
    [
        out("x4") _, out("x5") _, out("x6") _, out("x7") _,
        out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
        out("v6") _, out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
        out("v21") _, out("v22") _, out("v23") _,
    ],
    keys = sym ISCSI_V4_KEYS
);

asm_fn!(
    /// [`crc32c_v2_v4`] for CRC-32/ISO-HDLC.
    crc32_v2_v4,
    "crc,aes,sha3",
    v4_crc_tail_loop!("crc32x", "crc32w", "crc32h", "crc32b"),
    [
        out("x4") _, out("x5") _, out("x6") _, out("x7") _,
        out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
        out("v6") _, out("v16") _, out("v17") _, out("v18") _, out("v19") _, out("v20") _,
        out("v21") _, out("v22") _, out("v23") _,
    ],
    keys = sym ISO_HDLC_V4_KEYS
);

asm_fn!(
    /// CRC-32/ISCSI by Neoverse V2's eight-lane fold ([`v8_crc_tail_loop`]), for 128 bytes and
    /// more.
    crc32c_v2_v8,
    "crc,aes,sha3",
    v8_crc_tail_loop!("crc32cx", "crc32cw", "crc32ch", "crc32cb"),
    [
        out("x4") _, out("x5") _, out("x6") _, out("x7") _,
        out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
        out("v6") _, out("v7") _, out("v16") _, out("v17") _, out("v18") _, out("v19") _,
        out("v20") _, out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _,
        out("v26") _, out("v27") _, out("v28") _, out("v29") _, out("v30") _, out("v31") _,
    ],
    keys = sym ISCSI_V8_KEYS
);

asm_fn!(
    /// [`crc32c_v2_v8`] for CRC-32/ISO-HDLC.
    crc32_v2_v8,
    "crc,aes,sha3",
    v8_crc_tail_loop!("crc32x", "crc32w", "crc32h", "crc32b"),
    [
        out("x4") _, out("x5") _, out("x6") _, out("x7") _,
        out("v0") _, out("v1") _, out("v2") _, out("v3") _, out("v4") _, out("v5") _,
        out("v6") _, out("v7") _, out("v16") _, out("v17") _, out("v18") _, out("v19") _,
        out("v20") _, out("v21") _, out("v22") _, out("v23") _, out("v24") _, out("v25") _,
        out("v26") _, out("v27") _, out("v28") _, out("v29") _, out("v30") _, out("v31") _,
    ],
    keys = sym ISO_HDLC_V8_KEYS
);

/// CRC-32/ISCSI by Neoverse V2's four-lane fold, for 64 bytes and more, plus `xorout`.
///
/// # Safety
///
/// `buf` holds `len` readable bytes, `len` is at least 64, and the CPU has the CRC32
/// instructions, PMULL and SHA3.
#[inline(always)]
pub(super) unsafe fn crc32_iscsi_v2_v4(crc: u32, buf: *const u8, len: usize, xorout: u32) -> u32 {
    debug_assert!(len >= 64);
    // SAFETY: the caller holds `len` bytes at `buf`, at least 64, and checked the features
    unsafe { crc32c_v2_v4(crc, buf, len, xorout) }
}

/// [`crc32_iscsi_v2_v4`] for CRC-32/ISO-HDLC.
///
/// # Safety
///
/// As for [`crc32_iscsi_v2_v4`].
#[inline(always)]
pub(super) unsafe fn crc32_iso_hdlc_v2_v4(
    crc: u32,
    buf: *const u8,
    len: usize,
    xorout: u32,
) -> u32 {
    debug_assert!(len >= 64);
    // SAFETY: as for `crc32_iscsi_v2_v4`
    unsafe { crc32_v2_v4(crc, buf, len, xorout) }
}

/// CRC-32/ISCSI by Neoverse V2's eight-lane fold, for 128 bytes and more, plus `xorout`.
///
/// # Safety
///
/// `buf` holds `len` readable bytes, `len` is at least 128, and the CPU has the CRC32
/// instructions, PMULL and SHA3.
#[inline(always)]
pub(super) unsafe fn crc32_iscsi_v2_v8(crc: u32, buf: *const u8, len: usize, xorout: u32) -> u32 {
    debug_assert!(len >= 128);
    // SAFETY: the caller holds `len` bytes at `buf`, at least 128, and checked the features
    unsafe { crc32c_v2_v8(crc, buf, len, xorout) }
}

/// [`crc32_iscsi_v2_v8`] for CRC-32/ISO-HDLC.
///
/// # Safety
///
/// As for [`crc32_iscsi_v2_v8`].
#[inline(always)]
pub(super) unsafe fn crc32_iso_hdlc_v2_v8(
    crc: u32,
    buf: *const u8,
    len: usize,
    xorout: u32,
) -> u32 {
    debug_assert!(len >= 128);
    // SAFETY: as for `crc32_iscsi_v2_v8`
    unsafe { crc32_v2_v8(crc, buf, len, xorout) }
}

/// CRC-32/ISCSI of `data`, shorter than 64 bytes, from `crc`, plus `xorout`.
///
/// # Safety
///
/// The CPU has the CRC32 instructions, and `data` is shorter than 64 bytes.
#[inline(always)]
pub(super) unsafe fn crc32_iscsi_short(crc: u32, data: &[u8], xorout: u32) -> u32 {
    debug_assert!(data.len() < 64);
    // SAFETY: the caller checked for the CRC32 instructions; the loop reads `data`
    unsafe { crc32c_short(crc, data.as_ptr(), data.len(), xorout) }
}

/// CRC-32/ISO-HDLC of `data`, shorter than 64 bytes, from `crc`, plus `xorout`.
///
/// # Safety
///
/// As for [`crc32_iscsi_short`].
#[inline(always)]
pub(super) unsafe fn crc32_iso_hdlc_short(crc: u32, data: &[u8], xorout: u32) -> u32 {
    debug_assert!(data.len() < 64);
    // SAFETY: as for `crc32_iscsi_short`
    unsafe { crc32_short(crc, data.as_ptr(), data.len(), xorout) }
}

/// CRC-32/ISCSI of `data` by the CRC32 instruction alone, from `crc`, plus `xorout`.
///
/// # Safety
///
/// The CPU has the CRC32 instructions.
#[inline(always)]
pub(super) unsafe fn crc32_iscsi_loop(crc: u32, data: &[u8], xorout: u32) -> u32 {
    // SAFETY: the caller checked for the CRC32 instructions; the loop reads `data`
    unsafe { crc32c_blocks(crc, data.as_ptr(), data.len(), xorout) }
}

/// CRC-32/ISO-HDLC of `data` by the CRC32 instruction alone, from `crc`, plus `xorout`.
///
/// # Safety
///
/// As for [`crc32_iscsi_loop`].
#[inline(always)]
pub(super) unsafe fn crc32_iso_hdlc_loop(crc: u32, data: &[u8], xorout: u32) -> u32 {
    // SAFETY: as for `crc32_iscsi_loop`
    unsafe { crc32_blocks(crc, data.as_ptr(), data.len(), xorout) }
}

/// CRC-32/ISCSI by the four-lane fold, EOR3 form, for 64 bytes and more, plus `xorout`.
///
/// # Safety
///
/// `buf` holds `len` readable bytes, `len` is at least 64, and the CPU has the CRC32
/// instructions, PMULL and SHA3.
#[inline(always)]
pub(super) unsafe fn crc32_iscsi_eor3_v4(crc: u32, buf: *const u8, len: usize, xorout: u32) -> u32 {
    debug_assert!(len >= 64);
    // SAFETY: the caller holds `len` bytes at `buf`, at least 64, and checked the features
    unsafe { crc32c_eor3_v4(crc, buf, len, xorout) }
}

/// [`crc32_iscsi_eor3_v4`] for CRC-32/ISO-HDLC.
///
/// # Safety
///
/// As for [`crc32_iscsi_eor3_v4`].
#[inline(always)]
pub(super) unsafe fn crc32_iso_hdlc_eor3_v4(
    crc: u32,
    buf: *const u8,
    len: usize,
    xorout: u32,
) -> u32 {
    debug_assert!(len >= 64);
    // SAFETY: as for `crc32_iscsi_eor3_v4`
    unsafe { crc32_eor3_v4(crc, buf, len, xorout) }
}
