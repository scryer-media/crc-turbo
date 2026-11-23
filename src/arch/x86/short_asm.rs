// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.

//! The short-buffer folds of the x86 tiers, written as inline assembly so that their shape does
//! not depend on the compiler: the length tests they branch on, the products they fold onto
//! the last block, the registers they copy and the encoding they use are fixed by construction,
//! and a new LLVM can neither turn the ladder into a jump table nor spill the accumulators.
//!
//! Two lengths of ladder: 16 to 255 bytes for the SSE tier (legacy and AVX encodings), and 16
//! to 127 bytes for the wider tiers (AVX encoding), whose own folds take longer buffers. Their
//! 256-bit fold of 64 to 127 bytes was a call the compiler left out of line, and slower there
//! than these 128-bit arms. Each length has an arm of its own, straight-line, each block folded onto the last block across its own
//! distance so that the products are independent; from eight blocks the blocks before the
//! eighth from the end are folded onto it first by the same arms, the 112-byte key being the
//! farthest the generated table holds. The bytes after the last whole block are folded in
//! with an overlapping read; their code sits between arms, out of the way of whole blocks.
//! The result is reduced by the width's own reduction, compiled from intrinsics.
//!
//! Each body is in exactly one function, not generic and never inlined, per ladder, encoding,
//! width and bit order, and in each CRC-32/ISO-HDLC entry built here, so that each has a stable
//! path to check its assembly by. Under Miri, and on 32-bit x86, the same functions run the
//! intrinsics paths the tests check the assembly against.

#![cfg(any(target_arch = "x86", target_arch = "x86_64"))]

use crate::arch::x86::sse::X86SsePclmulqdqOps;
use crate::structs::{Width16, Width32, Width64};
use crate::CrcParams;

/// The index of the low key of the coefficient pair for folding across `d` blocks, at `d - 1`:
/// the pairs `algorithm::fold_blocks_onto_last` and the 128-byte fold's lane collapse use, a
/// pair's high key the one after its low key, so that the pair is one 16-byte load.
#[cfg(all(target_arch = "x86_64", not(miri)))]
const DISTANCE_KEY_LOW: [usize; 7] = [1, 19, 17, 15, 13, 11, 9];

/// Sixteen bytes of 0x80, flipping a byte-shuffle control between taking a byte and zeroing it.
#[cfg(all(target_arch = "x86_64", not(miri)))]
#[repr(align(16))]
// Read only by the assembly, through the symbol.
#[allow(dead_code)]
struct Aligned16([u8; 16]);

#[cfg(all(target_arch = "x86_64", not(miri)))]
static FLIP: Aligned16 = Aligned16([0x80; 16]);

/// The two carry-less multiply selectors of a 16-byte fold against a coefficient pair loaded
/// low key first: reflected, the low half by the high key and the high half by the low key.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! sel {
    (r32, hi) => {
        "0x10"
    };
    (r64, hi) => {
        "0x10"
    };
    (f, hi) => {
        "0x00"
    };
    (r32, lo) => {
        "0x01"
    };
    (r64, lo) => {
        "0x01"
    };
    (f, lo) => {
        "0x11"
    };
}

/// Whether the bit order is reflected.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! reflected {
    (f) => {
        false
    };
    ($o:ident) => {
        true
    };
}

/// A loaded block into the reflected domain, for the bit orders that need it.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! shuf {
    (sse, f, $r:literal) => {
        concat!("pshufb ", $r, ", {sm}\n")
    };
    (avx, f, $r:literal) => {
        concat!("vpshufb ", $r, ", ", $r, ", {sm}\n")
    };
    ($e:ident, $o:ident, $r:literal) => {
        ""
    };
}

/// `{s}` folded across the distance of the key pair at `{k} + {$key}`, in place.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! fold_s {
    (sse, $o:ident, $key:literal) => {
        concat!(
            "movdqu {kk}, xmmword ptr [{k} + {",
            $key,
            "}]\n",
            "movdqa {x}, {s}\n",
            "pclmulqdq {x}, {kk}, ",
            sel!($o, hi),
            "\n",
            "pclmulqdq {s}, {kk}, ",
            sel!($o, lo),
            "\n",
            "pxor {s}, {x}\n",
        )
    };
    (avx, $o:ident, $key:literal) => {
        concat!(
            "vpclmulqdq {x}, {s}, xmmword ptr [{k} + {",
            $key,
            "}], ",
            sel!($o, hi),
            "\n",
            "vpclmulqdq {s}, {s}, xmmword ptr [{k} + {",
            $key,
            "}], ",
            sel!($o, lo),
            "\n",
            "vpxor {s}, {s}, {x}\n",
        )
    };
}

/// `{s} ^=` the block at `$base + $off` folded across the distance of the key pair at
/// `{k} + {$key}`.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! fold_b {
    (sse, $o:ident, $base:literal, $off:literal, $key:literal) => {
        concat!(
            "movdqu {x}, xmmword ptr [",
            $base,
            " + ",
            $off,
            "]\n",
            shuf!(sse, $o, "{x}"),
            "movdqu {kk}, xmmword ptr [{k} + {",
            $key,
            "}]\n",
            "movdqa {y}, {x}\n",
            "pclmulqdq {x}, {kk}, ",
            sel!($o, hi),
            "\n",
            "pclmulqdq {y}, {kk}, ",
            sel!($o, lo),
            "\n",
            "pxor {s}, {x}\n",
            "pxor {s}, {y}\n",
        )
    };
    (avx, $o:ident, $base:literal, $off:literal, $key:literal) => {
        concat!(
            "vmovdqu {x}, xmmword ptr [",
            $base,
            " + ",
            $off,
            "]\n",
            shuf!(avx, $o, "{x}"),
            "vpclmulqdq {y}, {x}, xmmword ptr [{k} + {",
            $key,
            "}], ",
            sel!($o, hi),
            "\n",
            "vpclmulqdq {x}, {x}, xmmword ptr [{k} + {",
            $key,
            "}], ",
            sel!($o, lo),
            "\n",
            "vpxor {s}, {s}, {y}\n",
            "vpxor {s}, {s}, {x}\n",
        )
    };
}

/// `{s} ^=` the block at `$base + $off`.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! xor_b {
    (avx, f, $base:literal, $off:literal) => {
        concat!(
            "vmovdqu {x}, xmmword ptr [",
            $base,
            " + ",
            $off,
            "]\n",
            shuf!(avx, f, "{x}"),
            "vpxor {s}, {s}, {x}\n",
        )
    };
    (avx, $o:ident, $base:literal, $off:literal) => {
        concat!("vpxor {s}, {s}, xmmword ptr [", $base, " + ", $off, "]\n")
    };
    (sse, $o:ident, $base:literal, $off:literal) => {
        concat!(
            "movdqu {x}, xmmword ptr [",
            $base,
            " + ",
            $off,
            "]\n",
            shuf!(sse, $o, "{x}"),
            "pxor {s}, {x}\n",
        )
    };
}

/// The state `{s}`, the block at `$base` folded in, and the `$blocks - 1` blocks after it
/// folded onto the last of them, each across its own distance.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! arm {
    ($e:ident, $o:ident, $base:literal, 2) => {
        concat!(fold_s!($e, $o, "k1"), xor_b!($e, $o, $base, "16"))
    };
    ($e:ident, $o:ident, $base:literal, 3) => {
        concat!(
            fold_s!($e, $o, "k2"),
            fold_b!($e, $o, $base, "16", "k1"),
            xor_b!($e, $o, $base, "32"),
        )
    };
    ($e:ident, $o:ident, $base:literal, 4) => {
        concat!(
            fold_s!($e, $o, "k3"),
            fold_b!($e, $o, $base, "16", "k2"),
            fold_b!($e, $o, $base, "32", "k1"),
            xor_b!($e, $o, $base, "48"),
        )
    };
    ($e:ident, $o:ident, $base:literal, 5) => {
        concat!(
            fold_s!($e, $o, "k4"),
            fold_b!($e, $o, $base, "16", "k3"),
            fold_b!($e, $o, $base, "32", "k2"),
            fold_b!($e, $o, $base, "48", "k1"),
            xor_b!($e, $o, $base, "64"),
        )
    };
    ($e:ident, $o:ident, $base:literal, 6) => {
        concat!(
            fold_s!($e, $o, "k5"),
            fold_b!($e, $o, $base, "16", "k4"),
            fold_b!($e, $o, $base, "32", "k3"),
            fold_b!($e, $o, $base, "48", "k2"),
            fold_b!($e, $o, $base, "64", "k1"),
            xor_b!($e, $o, $base, "80"),
        )
    };
    ($e:ident, $o:ident, $base:literal, 7) => {
        concat!(
            fold_s!($e, $o, "k6"),
            fold_b!($e, $o, $base, "16", "k5"),
            fold_b!($e, $o, $base, "32", "k4"),
            fold_b!($e, $o, $base, "48", "k3"),
            fold_b!($e, $o, $base, "64", "k2"),
            fold_b!($e, $o, $base, "80", "k1"),
            xor_b!($e, $o, $base, "96"),
        )
    };
    ($e:ident, $o:ident, $base:literal, 8) => {
        concat!(
            fold_s!($e, $o, "k7"),
            fold_b!($e, $o, $base, "16", "k6"),
            fold_b!($e, $o, $base, "32", "k5"),
            fold_b!($e, $o, $base, "48", "k4"),
            fold_b!($e, $o, $base, "64", "k3"),
            fold_b!($e, $o, $base, "80", "k2"),
            fold_b!($e, $o, $base, "96", "k1"),
            xor_b!($e, $o, $base, "112"),
        )
    };
}

/// The encoding's instruction for a plain operation, two-operand or three-operand.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! op {
    (sse, loadu, $d:literal, $m:literal) => {
        concat!("movdqu ", $d, ", xmmword ptr ", $m, "\n")
    };
    (avx, loadu, $d:literal, $m:literal) => {
        concat!("vmovdqu ", $d, ", xmmword ptr ", $m, "\n")
    };
    (sse, mov, $d:literal, $s:literal) => {
        concat!("movdqa ", $d, ", ", $s, "\n")
    };
    (avx, mov, $d:literal, $s:literal) => {
        concat!("vmovdqa ", $d, ", ", $s, "\n")
    };
    (sse, xor, $d:literal, $s:literal) => {
        concat!("pxor ", $d, ", ", $s, "\n")
    };
    (avx, xor, $d:literal, $s:literal) => {
        concat!("vpxor ", $d, ", ", $d, ", ", $s, "\n")
    };
}

/// The control for the bytes after the last whole block: reflected, the table from the count
/// of them, forward, from sixteen less it.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! tail_control {
    ($e:ident, f) => {
        concat!("neg {m}\n", op!($e, loadu, "xmm0", "[{t} + {m} + 16]"))
    };
    ($e:ident, $o:ident) => {
        op!($e, loadu, "xmm0", "[{t} + {m}]")
    };
}

/// The bytes after the last whole block, read overlapping it into `{x}`, merged with the
/// folded state as `algorithm::get_last_two_xmms` merges them, and folded into `{s}`.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! tail {
    (sse, $o:ident) => {
        concat!(
            "movdqu {x}, xmmword ptr [{p} + {n} - 16]\n",
            shuf!(sse, $o, "{x}"),
            "mov {m}, {n}\n",
            "and {m}, 15\n",
            tail_control!(sse, $o),
            "movdqa {a}, {s}\n",
            "pshufb {a}, xmm0\n",
            "pxor xmm0, xmmword ptr [rip + {flip}]\n",
            "movdqa {y}, {s}\n",
            "pshufb {y}, xmm0\n",
            tail_merge!(sse, $o),
        )
    };
    (avx, $o:ident) => {
        concat!(
            "vmovdqu {x}, xmmword ptr [{p} + {n} - 16]\n",
            shuf!(avx, $o, "{x}"),
            "mov {m}, {n}\n",
            "and {m}, 15\n",
            tail_control!(avx, $o),
            "vpshufb {a}, {s}, xmm0\n",
            "vpxor xmm0, xmm0, xmmword ptr [rip + {flip}]\n",
            "vpshufb {y}, {s}, xmm0\n",
            tail_merge!(avx, $o),
        )
    };
}

/// `{a}` holds the state shuffled by the control, `{y}` by the flipped control in `xmm0`.
/// Widths of 32 bits and below, and the forward order, blend `{a}` into the read and fold
/// `{y}` onto it; reflected 64-bit blends the read into `{y}` and folds `{a}` onto it.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! tail_merge {
    (sse, r64) => {
        concat!(
            "pblendvb {y}, {x}, xmm0\n",
            "movdqu {kk}, xmmword ptr [{k} + {k1}]\n",
            "movdqa {s}, {a}\n",
            "pclmulqdq {a}, {kk}, ",
            sel!(r64, hi),
            "\n",
            "pclmulqdq {s}, {kk}, ",
            sel!(r64, lo),
            "\n",
            "pxor {s}, {a}\n",
            "pxor {s}, {y}\n",
        )
    };
    (sse, $o:ident) => {
        concat!(
            "pblendvb {x}, {a}, xmm0\n",
            "movdqu {kk}, xmmword ptr [{k} + {k1}]\n",
            "movdqa {s}, {y}\n",
            "pclmulqdq {y}, {kk}, ",
            sel!($o, hi),
            "\n",
            "pclmulqdq {s}, {kk}, ",
            sel!($o, lo),
            "\n",
            "pxor {s}, {y}\n",
            "pxor {s}, {x}\n",
        )
    };
    (avx, r64) => {
        concat!(
            "vpblendvb {y}, {y}, {x}, xmm0\n",
            "vpclmulqdq {s}, {a}, xmmword ptr [{k} + {k1}], ",
            sel!(r64, hi),
            "\n",
            "vpclmulqdq {a}, {a}, xmmword ptr [{k} + {k1}], ",
            sel!(r64, lo),
            "\n",
            "vpxor {s}, {s}, {a}\n",
            "vpxor {s}, {s}, {y}\n",
        )
    };
    (avx, $o:ident) => {
        concat!(
            "vpblendvb {x}, {x}, {a}, xmm0\n",
            "vpclmulqdq {s}, {y}, xmmword ptr [{k} + {k1}], ",
            sel!($o, hi),
            "\n",
            "vpclmulqdq {y}, {y}, xmmword ptr [{k} + {k1}], ",
            sel!($o, lo),
            "\n",
            "vpxor {s}, {s}, {y}\n",
            "vpxor {s}, {s}, {x}\n",
        )
    };
}

/// The fold of the bytes at `{p}`, `{n}` of them, 16 to 255 (`full`) or 16 to 127 (`h128`), onto
/// the state `{s}`, left in `{s}`.
///
/// A whole number of blocks runs its arm and jumps to the check for a tail at `30`; the
/// tail's code, at `38`, sits between arms and jumps to the end. From eight blocks, the arm
/// for the blocks up to the eighth from the end runs first, and the arm for eight blocks then
/// folds its result onto the last from `{r}`.
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! fold_asm {
    ($e:ident, full, $o:ident) => {
        concat!(
            op!($e, loadu, "{x}", "[{p}]"),
            shuf!($e, $o, "{x}"),
            op!($e, xor, "{s}", "{x}"),
            "cmp {n}, 32\n",
            "jb 30f\n",
            "cmp {n}, 128\n",
            "jae 50f\n",
            "test {n}, 64\n",
            "jnz 24f\n",
            "test {n}, 16\n",
            "jnz 23f\n",
            arm!($e, $o, "{p}", 2),
            "jmp 30f\n",
            "23:\n",
            arm!($e, $o, "{p}", 3),
            "jmp 30f\n",
            "38:\n",
            tail!($e, $o),
            "jmp 39f\n",
            "24:\n",
            "test {n}, 32\n",
            "jnz 26f\n",
            "test {n}, 16\n",
            "jnz 25f\n",
            arm!($e, $o, "{p}", 4),
            "jmp 30f\n",
            "25:\n",
            arm!($e, $o, "{p}", 5),
            "jmp 30f\n",
            "26:\n",
            "test {n}, 16\n",
            "jnz 27f\n",
            arm!($e, $o, "{p}", 6),
            "jmp 30f\n",
            "27:\n",
            arm!($e, $o, "{p}", 7),
            "jmp 30f\n",
            // Eight blocks or more: those up to the eighth from the end first, by the bits of
            // their count above eight.
            "50:\n",
            "test {n}, 64\n",
            "jnz 54f\n",
            "test {n}, 32\n",
            "jnz 52f\n",
            "test {n}, 16\n",
            "jz 58f\n",
            arm!($e, $o, "{p}", 2),
            "jmp 58f\n",
            "52:\n",
            "test {n}, 16\n",
            "jnz 53f\n",
            arm!($e, $o, "{p}", 3),
            "jmp 58f\n",
            "53:\n",
            arm!($e, $o, "{p}", 4),
            "jmp 58f\n",
            "54:\n",
            "test {n}, 32\n",
            "jnz 56f\n",
            "test {n}, 16\n",
            "jnz 55f\n",
            arm!($e, $o, "{p}", 5),
            "jmp 58f\n",
            "55:\n",
            arm!($e, $o, "{p}", 6),
            "jmp 58f\n",
            "56:\n",
            "test {n}, 16\n",
            "jnz 57f\n",
            arm!($e, $o, "{p}", 7),
            "jmp 58f\n",
            "57:\n",
            arm!($e, $o, "{p}", 8),
            "58:\n",
            "mov {r}, {n}\n",
            "and {r}, 112\n",
            "add {r}, {p}\n",
            arm!($e, $o, "{r}", 8),
            "30:\n",
            "test {n}, 15\n",
            "jnz 38b\n",
            "39:\n",
        )
    };
    ($e:ident, h128, $o:ident) => {
        concat!(
            op!($e, loadu, "{x}", "[{p}]"),
            shuf!($e, $o, "{x}"),
            op!($e, xor, "{s}", "{x}"),
            "cmp {n}, 32\n",
            "jb 30f\n",
            "test {n}, 64\n",
            "jnz 24f\n",
            "test {n}, 16\n",
            "jnz 23f\n",
            arm!($e, $o, "{p}", 2),
            "jmp 30f\n",
            "23:\n",
            arm!($e, $o, "{p}", 3),
            "jmp 30f\n",
            "38:\n",
            tail!($e, $o),
            "jmp 39f\n",
            "24:\n",
            "test {n}, 32\n",
            "jnz 26f\n",
            "test {n}, 16\n",
            "jnz 25f\n",
            arm!($e, $o, "{p}", 4),
            "jmp 30f\n",
            "25:\n",
            arm!($e, $o, "{p}", 5),
            "jmp 30f\n",
            "26:\n",
            "test {n}, 16\n",
            "jnz 27f\n",
            arm!($e, $o, "{p}", 6),
            "jmp 30f\n",
            "27:\n",
            arm!($e, $o, "{p}", 7),
            "jmp 30f\n",
            "30:\n",
            "test {n}, 15\n",
            "jnz 38b\n",
            "39:\n",
        )
    };
}

/// [`fold_asm`] with its operands: the byte-reversing mask for the forward order only, a
/// register for the keys for the two-operand encoding only, and the farther keys and the
/// second base for the full ladder only (the 128-byte ladder has no arm of eight blocks).
#[cfg(all(target_arch = "x86_64", not(miri)))]
macro_rules! fold_call {
    (@o $e:ident, $v:ident, f, [$($acc:tt)*] $s:ident, $p:expr, $n:expr, $k:expr, $t:expr,
     $sm:expr) => {
        fold_call!(@v $e, $v, f, [$($acc)* sm = in(xmm_reg) $sm,] $s, $p, $n, $k, $t)
    };
    (@o $e:ident, $v:ident, $o:ident, [$($acc:tt)*] $s:ident, $p:expr, $n:expr, $k:expr,
     $t:expr, $sm:expr) => {
        fold_call!(@v $e, $v, $o, [$($acc)*] $s, $p, $n, $k, $t)
    };
    (@v $e:ident, full, $o:ident, [$($acc:tt)*] $($rest:tt)*) => {
        fold_call!(@asm $e, full, $o, [$($acc)*
            k3 = const 8 * DISTANCE_KEY_LOW[2],
            k4 = const 8 * DISTANCE_KEY_LOW[3],
            k5 = const 8 * DISTANCE_KEY_LOW[4],
            k6 = const 8 * DISTANCE_KEY_LOW[5],
            k7 = const 8 * DISTANCE_KEY_LOW[6],
            r = out(reg) _,
        ] $($rest)*)
    };
    (@v $e:ident, h128, $o:ident, [$($acc:tt)*] $($rest:tt)*) => {
        fold_call!(@asm $e, h128, $o, [$($acc)*
            k3 = const 8 * DISTANCE_KEY_LOW[2],
            k4 = const 8 * DISTANCE_KEY_LOW[3],
            k5 = const 8 * DISTANCE_KEY_LOW[4],
            k6 = const 8 * DISTANCE_KEY_LOW[5],
        ] $($rest)*)
    };
    (@asm $e:ident, $v:ident, $o:ident, [$($acc:tt)*] $s:ident, $p:expr, $n:expr, $k:expr,
     $t:expr) => {
        // SAFETY: the caller has the encoding's features; every load is within the bytes,
        // the keys, the 32-byte shuffle table from `table` at an offset under 32, or `FLIP`.
        core::arch::asm!(
            fold_asm!($e, $v, $o),
            s = inout(xmm_reg) $s,
            p = in(reg) $p,
            n = in(reg) $n,
            k = in(reg) $k,
            t = in(reg) $t,
            flip = sym FLIP,
            k1 = const 8 * DISTANCE_KEY_LOW[0],
            k2 = const 8 * DISTANCE_KEY_LOW[1],
            $($acc)*
            a = out(xmm_reg) _,
            x = out(xmm_reg) _,
            y = out(xmm_reg) _,
            out("xmm0") _,
            m = out(reg) _,
            options(readonly, nostack),
        )
    };
    (sse, $v:ident, $o:ident, $($rest:tt)*) => {
        fold_call!(@o sse, $v, $o, [kk = out(xmm_reg) _,] $($rest)*)
    };
    (avx, $v:ident, $o:ident, $($rest:tt)*) => {
        fold_call!(@o avx, $v, $o, [] $($rest)*)
    };
}

/// The state `$state` of width `$w` over `$bytes`, 16 to 255 of them (`full`) or 16 to 127
/// (`h128`), with the parameters `$params` of bit order `$o` (`r32` reflected up to 32 bits,
/// `r64` reflected 64-bit, `f` forward), folded with encoding `$e` and reduced.
macro_rules! short_body {
    ($e:ident, $v:ident, $o:ident, $w:ty, $state:expr, $bytes:expr, $params:expr) => {{
        let (state, bytes, params): (_, &[u8], &CrcParams) = ($state, $bytes, $params);
        let len = bytes.len();
        debug_assert!((16..short_end!($v)).contains(&len));
        let ops = X86SsePclmulqdqOps;
        #[cfg(all(target_arch = "x86_64", not(miri)))]
        {
            use crate::traits::EnhancedCrcWidth;
            let reflected = reflected!($o);
            debug_assert_eq!(params.refin, reflected);
            // The fold reads its keys from memory in the assembly. The reduction takes them
            // as they are: hidden behind `opaque_keys`, a constant table's test of its Barrett
            // key became a load and a branch after the fold, and its keys came from memory.
            let keys = params.keys.fold_keys();
            let mut s = <$w>::create_state(state, reflected, &ops).value;
            let (table, _) = <$w>::get_last_bytes_table_ptr(reflected, 0);
            fold_call!(
                $e,
                $v,
                $o,
                s,
                bytes.as_ptr(),
                len,
                keys.as_ptr(),
                table,
                <X86SsePclmulqdqOps as crate::traits::ArchOps>::load_aligned(
                    &ops,
                    &<$w>::load_constants(false)[0] as *const [u64; 2]
                )
            );
            <$w>::perform_final_reduction(s, reflected, keys, &ops)
        }
        #[cfg(not(all(target_arch = "x86_64", not(miri))))]
        {
            if len < 128 {
                crate::algorithm::update_short_ladder::<_, $w>(state, bytes, params, &ops)
            } else {
                crate::algorithm::update_mid::<_, $w, false>(state, bytes, params, &ops)
            }
        }
    }};
}

/// Where each ladder ends.
macro_rules! short_end {
    (full) => {
        256
    };
    (h128) => {
        128
    };
}

/// The call to the function of `$r16` to `$f64` for the parameters' width and bit order.
macro_rules! by_params {
    ($state:expr, $bytes:expr, $params:expr, $r16:ident, $r32:ident, $r64:ident, $f16:ident,
     $f32:ident, $f64:ident) => {{
        let (state, bytes, params): (u64, &[u8], &CrcParams) = ($state, $bytes, $params);
        match (params.width, params.refin) {
            (64, true) => $r64(state, bytes, params),
            (64, false) => $f64(state, bytes, params),
            (32, true) => $r32(state, bytes, params),
            (32, false) => $f32(state, bytes, params),
            (16, true) => $r16(state, bytes, params),
            (16, false) => $f16(state, bytes, params),
            _ => panic!("Unsupported CRC width: {}", params.width),
        }
    }};
}

/// One ladder for one encoding: a function per width and bit order, a dispatcher on the
/// parameters.
macro_rules! short_fns {
    ($features:literal, $e:ident, $v:ident, $dispatch:ident, $r16:ident, $r32:ident,
     $r64:ident, $f16:ident, $f32:ident, $f64:ident) => {
        short_fns!(@one $features, $e, $v, r32, Width16, u16, $r16);
        short_fns!(@one $features, $e, $v, r32, Width32, u32, $r32);
        short_fns!(@one $features, $e, $v, r64, Width64, u64, $r64);
        short_fns!(@one $features, $e, $v, f, Width16, u16, $f16);
        short_fns!(@one $features, $e, $v, f, Width32, u32, $f32);
        short_fns!(@one $features, $e, $v, f, Width64, u64, $f64);

        /// The state over the ladder's lengths, through the function for the parameters'
        /// width and bit order.
        ///
        /// # Safety
        ///
        /// The CPU must have the encoding's features, and `bytes` must be within the ladder.
        #[inline(always)]
        pub(crate) unsafe fn $dispatch(state: u64, bytes: &[u8], params: &CrcParams) -> u64 {
            by_params!(state, bytes, params, $r16, $r32, $r64, $f16, $f32, $f64)
        }
    };
    (@one $features:literal, $e:ident, $v:ident, $o:ident, $w:ty, $t:ty, $name:ident) => {
        /// The width's state over the ladder's lengths, for parameters of the bit order in
        /// the name.
        ///
        /// # Safety
        ///
        /// The CPU must have the encoding's features, `bytes` must be within the ladder, and
        /// `params` must have the width and bit order in the name.
        #[inline(never)]
        #[target_feature(enable = $features)]
        pub(crate) unsafe fn $name(state: u64, bytes: &[u8], params: &CrcParams) -> u64 {
            debug_assert!(params.width as usize == core::mem::size_of::<$t>() * 8);
            short_body!($e, $v, $o, $w, state as $t, bytes, params) as u64
        }
    };
}

/// CRC-32/ISO-HDLC's update entry over a ladder, for one encoding.
macro_rules! iso_hdlc_update {
    ($features:literal, $e:ident, $v:ident, $iso_hdlc:ident) => {
        /// CRC-32/ISO-HDLC over the ladder's lengths, in the published form, from the
        /// published CRC so far, `crc`.
        ///
        /// # Safety
        ///
        /// The CPU must have the encoding's features, and `bytes` must be within the ladder.
        #[inline(never)]
        #[target_feature(enable = $features)]
        pub(crate) unsafe fn $iso_hdlc(crc: u32, bytes: &[u8]) -> u64 {
            !short_body!(
                $e,
                $v,
                r32,
                Width32,
                !crc,
                bytes,
                &crate::crc32::consts::CRC32_ISO_HDLC
            ) as u64
        }
    };
}

/// A tier's CRC-32/ISO-HDLC checksum entry over a ladder, with the tier's features: from
/// 16 bytes to the ladder's end the ladder, after it the tier's own short ladder up to 128
/// bytes and its folds from there.
macro_rules! iso_hdlc_checksum {
    ($features:literal, $e:ident, $v:ident, $checksum:ident, $mid:ident, $fold:ident,
     $ops:expr) => {
        /// The CRC-32/ISO-HDLC checksum entry: the ladder in line from no CRC, so that the
        /// initial state is a constant, and from the ladder's end a jump to the tier's folds.
        ///
        /// # Safety
        ///
        /// The CPU must have the tier's features.
        #[inline(never)]
        #[target_feature(enable = $features)]
        pub(crate) unsafe fn $checksum(bytes: &[u8]) -> u64 {
            let params = &crate::crc32::consts::CRC32_ISO_HDLC;
            let len = bytes.len();
            if len >= 16 {
                if len < short_end!($v) {
                    return !short_body!($e, $v, r32, Width32, !0u32, bytes, params) as u64;
                }
                if len >= 128 {
                    if len >= crate::arch::MID_END {
                        return crate::arch::$fold(0, bytes);
                    }
                    return crate::arch::$mid(0, bytes);
                }
                return !crate::algorithm::update_short_ladder::<_, Width32>(
                    !0, bytes, params, &$ops,
                ) as u64;
            }
            !crate::algorithm::update_short::<_, Width32>(!0, bytes, params, &$ops) as u64
        }
    };
}

short_fns!(
    "sse4.1,pclmulqdq",
    sse,
    full,
    short_sse,
    short_sse_r16,
    short_sse_r32,
    short_sse_r64,
    short_sse_f16,
    short_sse_f32,
    short_sse_f64
);
iso_hdlc_update!("sse4.1,pclmulqdq", sse, full, iso_hdlc_short_sse);
iso_hdlc_checksum!(
    "sse4.1,pclmulqdq",
    sse,
    full,
    iso_hdlc_checksum_sse,
    iso_hdlc_mid_x86_sse_pclmulqdq,
    iso_hdlc_fold_x86_sse_pclmulqdq,
    X86SsePclmulqdqOps
);

iso_hdlc_update!("sse4.1,pclmulqdq,avx", avx, full, iso_hdlc_short_avx);
iso_hdlc_checksum!(
    "sse4.1,pclmulqdq,avx",
    avx,
    full,
    iso_hdlc_checksum_avx,
    iso_hdlc_mid_x86_sse_avx,
    iso_hdlc_fold_x86_sse_avx,
    X86SsePclmulqdqOps
);

#[cfg(target_arch = "x86_64")]
short_fns!(
    "sse4.1,pclmulqdq,avx",
    avx,
    h128,
    short128_avx,
    short128_avx_r16,
    short128_avx_r32,
    short128_avx_r64,
    short128_avx_f16,
    short128_avx_f32,
    short128_avx_f64
);
#[cfg(target_arch = "x86_64")]
iso_hdlc_update!("sse4.1,pclmulqdq,avx", avx, h128, iso_hdlc_short128_avx);
#[cfg(target_arch = "x86_64")]
iso_hdlc_checksum!(
    "avx512vl,vpclmulqdq",
    avx,
    h128,
    iso_hdlc_checksum_avx512_vpclmulqdq,
    iso_hdlc_mid_x86_64_avx512_vpclmulqdq,
    iso_hdlc_fold_x86_64_avx512_vpclmulqdq,
    crate::arch::x86_64::avx512_vpclmulqdq::X86_64Avx512VpclmulqdqOps::new()
);
#[cfg(target_arch = "x86_64")]
iso_hdlc_checksum!(
    "avx512vl,pclmulqdq",
    avx,
    h128,
    iso_hdlc_checksum_avx512_pclmulqdq,
    iso_hdlc_mid_x86_64_avx512_pclmulqdq,
    iso_hdlc_fold_x86_64_avx512_pclmulqdq,
    crate::arch::x86_64::avx512::X86_64Avx512PclmulqdqOps::new()
);
#[cfg(target_arch = "x86_64")]
iso_hdlc_checksum!(
    "avx2,vpclmulqdq,sse4.1,pclmulqdq",
    avx,
    h128,
    iso_hdlc_checksum_avx2_vpclmulqdq,
    iso_hdlc_mid_x86_64_avx2_vpclmulqdq,
    iso_hdlc_fold_x86_64_avx2_vpclmulqdq,
    crate::arch::x86_64::avx2_vpclmulqdq::X86_64Avx2VpclmulqdqOps::new()
);

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::test::consts::TEST_ALL_CONFIGS;
    use rand::{rng, RngExt};

    type Short = unsafe fn(u64, &[u8], &CrcParams) -> u64;

    /// Every length of a ladder, from four starts, from the init state and a carried one, for
    /// every algorithm, through the family's function for it, and CRC-32/ISO-HDLC through the
    /// ladder's update entry, against the software path.
    fn check(end: usize, fns: [Short; 6], iso_hdlc: unsafe fn(u32, &[u8]) -> u64) {
        let mut data = vec![0u8; 512];
        rng().fill(&mut data[..]);
        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let f = match (params.width, params.refin) {
                (16, true) => fns[0],
                (32, true) => fns[1],
                (64, true) => fns[2],
                (16, false) => fns[3],
                (32, false) => fns[4],
                (64, false) => fns[5],
                _ => unreachable!(),
            };
            let init = config.get_init_algorithm();
            let carried = crate::arch::software::update(init, &data[300..307], params);
            for offset in [0usize, 1, 15, 33] {
                for len in 16..end {
                    let input = &data[offset..offset + len];
                    for state in [init, carried] {
                        let expected = crate::arch::software::update(state, input, params);
                        let actual = unsafe { f(state, input, params) };
                        assert_eq!(
                            actual,
                            expected,
                            "{} offset {offset} len {len} state {state:#x}",
                            config.get_name()
                        );
                    }
                }
            }
        }
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        for offset in [0usize, 1, 15, 33] {
            for len in 16..end {
                let input = &data[offset..offset + len];
                for crc in [0u32, 0x1234_5678] {
                    let expected =
                        !crate::arch::software::update(!crc as u64, input, params) & 0xffff_ffff;
                    assert_eq!(
                        unsafe { iso_hdlc(crc, input) },
                        expected,
                        "CRC-32/ISO-HDLC offset {offset} len {len} crc {crc:#x}"
                    );
                }
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_short_sse_matches_software() {
        if !(is_x86_feature_detected!("sse4.1") && is_x86_feature_detected!("pclmulqdq")) {
            eprintln!("skipping: the CPU lacks SSE4.1 or PCLMULQDQ");
            return;
        }
        check(
            256,
            [
                short_sse_r16,
                short_sse_r32,
                short_sse_r64,
                short_sse_f16,
                short_sse_f32,
                short_sse_f64,
            ],
            iso_hdlc_short_sse,
        );
    }

    /// The AVX encoding: the 16 to 255-byte ladder through CRC-32/ISO-HDLC's update entry
    /// alone, which is all of it that is built, and the 16 to 127-byte one whole.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_short_avx_matches_software() {
        if !(is_x86_feature_detected!("sse4.1")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("avx"))
        {
            eprintln!("skipping: the CPU lacks SSE4.1, PCLMULQDQ or AVX");
            return;
        }
        check(
            256,
            [
                short_sse_r16,
                short_sse_r32,
                short_sse_r64,
                short_sse_f16,
                short_sse_f32,
                short_sse_f64,
            ],
            iso_hdlc_short_avx,
        );
        #[cfg(target_arch = "x86_64")]
        check(
            128,
            [
                short128_avx_r16,
                short128_avx_r32,
                short128_avx_r64,
                short128_avx_f16,
                short128_avx_f32,
                short128_avx_f64,
            ],
            iso_hdlc_short128_avx,
        );
    }
}
