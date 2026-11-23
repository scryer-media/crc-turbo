//! The multipliers the three-stream kernels shift their partial CRCs by, built at compile time.
//!
//! At the end of a buffer the `v9s3x2e_s3` kernels move each scalar stream's CRC past the bytes
//! that follow it, multiplying it by `x^(8 * nbytes - 33) mod P`. Working that out by repeated
//! squaring, three times a call, cost more than the kernel's fold for buffers of a few KiB. Every
//! distance the kernels use is a whole number of 64-bit words, so up to `MAX_WORDS` words it is a
//! table lookup. A longer distance is `w = h * MAX_WORDS + lo` words, and its multiplier the
//! product of `x^(64 * lo - 33)` and the long table's `x^(64 * MAX_WORDS * h - 33)`: one carry-less
//! multiply, and one CRC32 instruction, whose multiplication by `x^33` cancels the second `-33`.
//! Past the long table, at 16 MiB, the squaring is a small part of the buffer's time and stays.

#![cfg(target_arch = "aarch64")]

use crate::crc32::consts::{CRC32_ISCSI, CRC32_ISO_HDLC};

/// The longest distance the tables cover, in 64-bit words: 16 KiB.
pub(crate) const MAX_WORDS: usize = 2048;

/// `ISCSI[w]` is `x^(64 * w - 33) mod P` for CRC-32/ISCSI, for `w` from 1; entry 0 is unused.
pub(crate) static ISCSI: [u32; MAX_WORDS + 1] = table((CRC32_ISCSI.poly as u32).reverse_bits());

/// The same for CRC-32/ISO-HDLC.
pub(crate) static ISO_HDLC: [u32; MAX_WORDS + 1] =
    table((CRC32_ISO_HDLC.poly as u32).reverse_bits());

/// Entries in the long tables; `ISCSI_LONG[h]` covers distances of `h` times `MAX_WORDS` words.
pub(crate) const LONG_LEN: usize = 1024;

/// `ISCSI_LONG[h]` is `x^(64 * MAX_WORDS * h - 33) mod P` for CRC-32/ISCSI, for `h` from 1.
pub(crate) static ISCSI_LONG: [u32; LONG_LEN] =
    long_table((CRC32_ISCSI.poly as u32).reverse_bits());

/// The same for CRC-32/ISO-HDLC.
pub(crate) static ISO_HDLC_LONG: [u32; LONG_LEN] =
    long_table((CRC32_ISO_HDLC.poly as u32).reverse_bits());

/// `a * x mod P`, in the reflected order where `x^0` is the top bit, `poly` reflected.
const fn times_x(a: u32, poly: u32) -> u32 {
    (a >> 1) ^ (poly & 0u32.wrapping_sub(a & 1))
}

/// `a * b mod P`.
const fn times(mut a: u32, b: u32, poly: u32) -> u32 {
    let mut product = 0u32;
    let mut bit = 0;
    while bit < 32 {
        if (b >> (31 - bit)) & 1 == 1 {
            product ^= a;
        }
        a = times_x(a, poly);
        bit += 1;
    }
    product
}

/// `x^n mod P`.
const fn x_to_the(mut n: u64, poly: u32) -> u32 {
    let mut result = 1u32 << 31;
    let mut square = 1u32 << 30;
    while n != 0 {
        if n & 1 == 1 {
            result = times(result, square, poly);
        }
        square = times(square, square, poly);
        n >>= 1;
    }
    result
}

const fn table(poly: u32) -> [u32; MAX_WORDS + 1] {
    let mut table = [0u32; MAX_WORDS + 1];
    let mut power = x_to_the(31, poly);
    let mut w = 1;
    while w <= MAX_WORDS {
        table[w] = power;
        let mut bit = 0;
        while bit < 64 {
            power = times_x(power, poly);
            bit += 1;
        }
        w += 1;
    }
    table
}

const fn long_table(poly: u32) -> [u32; LONG_LEN] {
    let mut table = [0u32; LONG_LEN];
    let step = x_to_the(64 * MAX_WORDS as u64, poly);
    let mut power = x_to_the(64 * MAX_WORDS as u64 - 33, poly);
    let mut h = 1;
    while h < LONG_LEN {
        table[h] = power;
        power = times(power, step, poly);
        h += 1;
    }
    table
}

/// The pair of multipliers that folds a 128-bit lane `distance` bytes forward, in the order the
/// kernels load them: `x^(8 * distance + 31)` for the low half, `x^(8 * distance - 33)` for the
/// high half, both mod P. This is the fast-crc32 generator's key, so a lane folded with it ends
/// in the same `x^32`-scaled form the final two CRC32 instructions reduce.
pub(crate) const fn fold_keys(distance: usize, poly: u32) -> [u64; 2] {
    let bits = 8 * distance as u64;
    [
        x_to_the(bits + 31, poly) as u64,
        x_to_the(bits - 33, poly) as u64,
    ]
}

/// [`fold_keys`] for CRC-32/ISCSI.
pub(crate) const fn iscsi_fold_keys(distance: usize) -> [u64; 2] {
    fold_keys(distance, (CRC32_ISCSI.poly as u32).reverse_bits())
}

/// [`fold_keys`] for CRC-32/ISO-HDLC.
pub(crate) const fn iso_hdlc_fold_keys(distance: usize) -> [u64; 2] {
    fold_keys(distance, (CRC32_ISO_HDLC.poly as u32).reverse_bits())
}

/// Where the multiplier for a distance comes from.
pub(crate) enum Shift {
    /// The short table's entry.
    Exact(u32),
    /// The product of these two entries, a short one and a long one.
    Product(u32, u32),
    /// Past both tables.
    Beyond,
}

/// The multiplier for `nbytes`, a whole number of 64-bit words, from the short `table` and the
/// `long` one.
#[inline(always)]
pub(crate) fn lookup(table: &[u32; MAX_WORDS + 1], long: &[u32; LONG_LEN], nbytes: usize) -> Shift {
    debug_assert!(nbytes >= 8 && nbytes.is_multiple_of(8));
    let words = nbytes / 8;
    if let Some(&shift) = table.get(words) {
        return Shift::Exact(shift);
    }
    let h = (words - 1) / MAX_WORDS;
    match long.get(h) {
        Some(&far) => Shift::Product(table[words - h * MAX_WORDS], far),
        None => Shift::Beyond,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The derived fold keys are the ones the fast-crc32 generator printed for the v12e kernels,
    /// for the 192-byte stride and the 16-, 32- and 64-byte reduction steps.
    #[test]
    fn fold_keys_match_the_generator() {
        assert_eq!(iscsi_fold_keys(192), [0xa87ab8a8, 0xab7aff2a]);
        assert_eq!(iscsi_fold_keys(16), [0xf20c0dfe, 0x493c7d27]);
        assert_eq!(iscsi_fold_keys(32), [0x3da6d0cb, 0xba4fc28e]);
        assert_eq!(iscsi_fold_keys(64), [0x740eef02, 0x9e4addf8]);
        assert_eq!(iscsi_fold_keys(128), [0x6992cea2, 0x0d3b6092]);
        assert_eq!(iso_hdlc_fold_keys(192), [0x596c8d81, 0xf5e48c85]);
        assert_eq!(iso_hdlc_fold_keys(16), [0xae689191, 0xccaa009e]);
        assert_eq!(iso_hdlc_fold_keys(32), [0xf1da05aa, 0x81256527]);
        assert_eq!(iso_hdlc_fold_keys(64), [0x8f352d95, 0x1d9513d7]);
    }

    /// Every entry is the power the generic definition gives, checked by an independent route:
    /// multiplying `x^31` by `x` one bit at a time.
    #[test]
    fn entries_are_the_powers_of_x() {
        for (table, poly) in [
            (&ISCSI, (CRC32_ISCSI.poly as u32).reverse_bits()),
            (&ISO_HDLC, (CRC32_ISO_HDLC.poly as u32).reverse_bits()),
        ] {
            for w in [1usize, 2, 3, 17, 100, 1000, MAX_WORDS] {
                assert_eq!(table[w], x_to_the(64 * w as u64 - 33, poly), "word {w}");
            }
        }
        for (table, poly) in [
            (&ISCSI_LONG, (CRC32_ISCSI.poly as u32).reverse_bits()),
            (&ISO_HDLC_LONG, (CRC32_ISO_HDLC.poly as u32).reverse_bits()),
        ] {
            for h in [1usize, 2, 7, LONG_LEN - 1] {
                let n = 64 * (MAX_WORDS * h) as u64 - 33;
                assert_eq!(table[h], x_to_the(n, poly), "long entry {h}");
            }
        }
    }
}
