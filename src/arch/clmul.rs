// Copyright 2026 the crc-turbo authors. Licensed under MIT or Apache-2.0.

//! The carry-less multiply behind the combine operator's field arithmetic: one PCLMULQDQ or
//! PMULL product and a Barrett reduction, reached only through [`Clmul`], which exists only
//! after detection.

/// Proof that the selected tier has a carry-less multiply instruction.
///
/// Built only by [`Clmul::detect`], so [`Clmul::mul_mod`] runs its intrinsic only on a CPU that
/// detection has selected a tier with the instruction for. On other architectures the type is
/// uninhabited and `detect` is always `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) struct Clmul(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) enum Clmul {}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
impl Clmul {
    #[inline]
    pub(crate) fn detect() -> Option<Self> {
        None
    }

    #[inline]
    pub(crate) fn mul_mod(self, _a: u64, _b: u64, _poly: u64, _mu: u64, _width: u8) -> u64 {
        match self {}
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
impl Clmul {
    /// `Some` when the selected tier has the instruction.
    ///
    /// Follows the selected tier, so the software tier (detected or forced) keeps combining in
    /// software too. Every accelerated x86_64 tier requires PCLMULQDQ and every aarch64 one
    /// requires PMULL (`tier_supported` in `feature_detection`), which is what makes the
    /// instruction safe to run.
    #[inline]
    pub(crate) fn detect() -> Option<Self> {
        use crate::feature_detection::{get_arch_ops, PerformanceTier};

        match get_arch_ops().get_tier() {
            PerformanceTier::SoftwareTable => None,
            _ => Some(Self(())),
        }
    }

    /// `a * b mod P(x)` for the polynomial `poly` (without its leading `x^width` term) of degree
    /// `width`, with `mu` its Barrett constant `floor(x^(2 * width) / P(x))`.
    #[inline]
    pub(crate) fn mul_mod(self, a: u64, b: u64, poly: u64, mu: u64, width: u8) -> u64 {
        // SAFETY: `self` exists only when the tier selected after detection has the instruction
        // `mul_mod_clmul` is compiled for.
        unsafe { mul_mod_clmul(a, b, poly, mu, width) }
    }
}

/// The full 128-bit carry-less product of two 64-bit polynomials.
///
/// # Safety
/// The CPU must support PCLMULQDQ.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "pclmulqdq,sse2")]
unsafe fn clmul(a: u64, b: u64) -> u128 {
    use core::arch::x86_64::*;

    let product = _mm_clmulepi64_si128(_mm_set_epi64x(0, a as i64), _mm_set_epi64x(0, b as i64), 0);
    let low = _mm_cvtsi128_si64(product) as u64;
    let high = _mm_cvtsi128_si64(_mm_srli_si128::<8>(product)) as u64;

    ((high as u128) << 64) | low as u128
}

/// The full 128-bit carry-less product of two 64-bit polynomials.
///
/// # Safety
/// The CPU must support PMULL (the `aes` feature).
#[cfg(target_arch = "aarch64")]
#[inline]
#[target_feature(enable = "aes")]
unsafe fn clmul(a: u64, b: u64) -> u128 {
    core::arch::aarch64::vmull_p64(a, b)
}

/// `a * b mod P(x)` by a carry-less multiply and a Barrett reduction.
///
/// With `T = a * b` (degree below `2 * width - 1`), `upper = floor(T / x^width)` and
/// `mu = floor(x^(2 * width) / P(x))`, the quotient `floor(T / P(x))` is exactly
/// `floor(upper * mu / x^width)`: GF(2) has no carries for the discarded low terms to lift. The
/// remainder is then the low `width` bits of `T + quotient * P(x)`, where only `poly` reaches
/// them.
///
/// # Safety
/// The CPU must support the carry-less multiply `clmul` is compiled for.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline]
#[cfg_attr(target_arch = "x86_64", target_feature(enable = "pclmulqdq,sse2"))]
#[cfg_attr(target_arch = "aarch64", target_feature(enable = "aes"))]
unsafe fn mul_mod_clmul(a: u64, b: u64, poly: u64, mu: u64, width: u8) -> u64 {
    let mask = u64::MAX >> (64 - u32::from(width));
    let width = u32::from(width);

    let product = clmul(a, b);
    let upper = (product >> width) as u64;
    let lower = product as u64 & mask;

    // mu's implicit x^width term contributes `upper` itself.
    let quotient = (clmul(upper, mu) >> width) as u64 ^ upper;

    lower ^ (clmul(quotient, poly) as u64 & mask)
}
