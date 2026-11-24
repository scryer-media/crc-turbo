//! Powers of `x` modulo a CRC-32 polynomial, worked out at compile time: the fold keys and
//! shift multipliers of the fusion kernels, so that none of them is a pasted literal.

#![cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]

use crate::crc32::consts::CRC32_ISCSI;
#[cfg(target_arch = "aarch64")]
use crate::crc32::consts::CRC32_ISO_HDLC;

/// `a * x mod P`, in the reflected order where `x^0` is the top bit, `poly` reflected.
pub(crate) const fn times_x(a: u32, poly: u32) -> u32 {
    (a >> 1) ^ (poly & 0u32.wrapping_sub(a & 1))
}

/// `a * b mod P`.
pub(crate) const fn times(mut a: u32, b: u32, poly: u32) -> u32 {
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
pub(crate) const fn x_to_the(mut n: u64, poly: u32) -> u32 {
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
#[cfg(target_arch = "aarch64")]
pub(crate) const fn iso_hdlc_fold_keys(distance: usize) -> [u64; 2] {
    fold_keys(distance, (CRC32_ISO_HDLC.poly as u32).reverse_bits())
}
