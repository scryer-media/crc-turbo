// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

#![allow(dead_code)]

use crate::traits::{CrcCalculator, CrcWidth};
use crate::{arch, cache, CrcAlgorithm, CrcError, CrcParams};

/// CRC algorithm parameters matching the CRC catalogue specification.
///
/// This struct describes a CRC algorithm using the fields specified by the
/// [Catalogue of parametrised CRC algorithms](https://reveng.sourceforge.io/crc-catalogue/all.htm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Algorithm<W> {
    /// The number of bit cells in the linear feedback shift register; the degree of the generator
    /// polynomial, minus one.
    pub width: u8,
    /// The generator polynomial that sets the feedback tap positions of the shift register.
    pub poly: W,
    /// The settings of the bit cells at the start of each calculation, before reading the first
    /// message bit.
    pub init: W,
    /// If `true`, characters are read bit-by-bit, least significant bit (LSB) first;
    /// if `false`, most significant bit (MSB) first.
    pub refin: bool,
    /// If `true`, the contents of the register after reading the last message bit are reflected
    /// before presentation; if `false`, they are unreflected.
    pub refout: bool,
    /// The XOR value applied to the contents of the register after the last message bit has been
    /// read and after the optional reflection.
    pub xorout: W,
    /// The contents of the register after initialising, reading the UTF-8 string `"123456789"`,
    /// optionally reflecting, and applying the final XOR.
    pub check: W,
    /// The contents of the register after initialising, reading an error-free codeword and
    /// optionally reflecting the register, but not applying the final XOR.
    pub residue: W,
}

/// CRC-16 width implementation
#[derive(Clone, Copy)]
pub struct Width16;

impl CrcWidth for Width16 {
    const WIDTH: u32 = 16;
    type Value = u16;
}

/// CRC-32 width implementation
#[derive(Clone, Copy)]
pub struct Width32;

impl CrcWidth for Width32 {
    const WIDTH: u32 = 32;
    type Value = u32;
}

/// CRC-64 width implementation
#[derive(Clone, Copy)]
pub struct Width64;

impl CrcWidth for Width64 {
    const WIDTH: u32 = 64;
    type Value = u64;
}

/// CRC State wrapper to manage the SIMD operations and reflection mode
#[derive(Debug, Clone, Copy)]
pub struct CrcState<T> {
    pub value: T,
    pub reflected: bool,
}

pub(crate) struct Calculator {}

impl CrcCalculator for Calculator {
    #[inline(always)]
    fn calculate(state: u64, data: &[u8], params: &CrcParams) -> u64 {
        unsafe { arch::update(state, data, params) }
    }
}

impl CrcParams {
    /// Creates custom CRC parameters for a given set of Rocksoft CRC parameters.
    ///
    /// Uses an internal cache to avoid regenerating folding keys for identical parameter sets.
    /// The first call with a given set of parameters will generate and cache the keys, while
    /// subsequent calls with the same parameters will use the cached keys for optimal performance.
    ///
    /// Does not support mis-matched refin/refout parameters, so both must be true or both false.
    ///
    /// Rocksoft parameters for lots of variants: <https://reveng.sourceforge.io/crc-catalogue/all.htm>
    ///
    /// # Panics
    ///
    /// If `width` is not 16, 32 or 64; [`CrcParams::try_new`] returns the error instead.
    pub fn new(
        name: &'static str,
        width: u8,
        poly: u64,
        init: u64,
        reflected: bool,
        xorout: u64,
        check: u64,
    ) -> Self {
        match Self::try_new(name, width, poly, init, reflected, xorout, check) {
            Ok(params) => params,
            Err(error) => panic!("{error}"),
        }
    }

    /// [`CrcParams::new`] that returns [`CrcError::UnsupportedWidth`] instead of panicking.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use crc_turbo::{checksum_with_params, CrcError, CrcParams};
    ///
    /// let params = CrcParams::try_new("CRC-32/CUSTOM", 32, 0x04c11db7, 0xffffffff, true, 0xffffffff, 0xcbf43926)
    ///     .expect("a supported width");
    /// assert_eq!(checksum_with_params(params, b"123456789"), 0xcbf43926);
    ///
    /// let error = CrcParams::try_new("CRC-8/CUSTOM", 8, 0x07, 0, false, 0, 0xf4).unwrap_err();
    /// assert_eq!(error, CrcError::UnsupportedWidth(8));
    /// ```
    pub fn try_new(
        name: &'static str,
        width: u8,
        poly: u64,
        init: u64,
        reflected: bool,
        xorout: u64,
        check: u64,
    ) -> Result<Self, CrcError> {
        // Checked before the keys are generated, which the generator cannot do for another width.
        if !matches!(width, 16 | 32 | 64) {
            return Err(CrcError::UnsupportedWidth(width));
        }

        let keys_array = cache::get_or_generate_keys(width, poly, reflected);
        let keys = crate::CrcKeysStorage::from_keys_fold_256(keys_array);

        let init_algorithm = initial_state(width, init, reflected);

        Ok(Self {
            algorithm: CrcAlgorithm::CrcCustom,
            name,
            width,
            poly,
            init,
            init_algorithm,
            refin: reflected,
            refout: reflected,
            xorout,
            check,
            keys,
        })
    }

    /// Gets a key at the specified index, returning 0 if out of bounds.
    /// This provides safe access regardless of internal key storage format.
    #[inline(always)]
    pub fn get_key(&self, index: usize) -> u64 {
        self.keys.get_key(index)
    }

    /// Gets a key at the specified index, returning None if out of bounds.
    /// This provides optional key access for cases where bounds checking is needed.
    #[inline(always)]
    pub fn get_key_checked(&self, index: usize) -> Option<u64> {
        if index < self.keys.key_count() {
            Some(self.keys.get_key(index))
        } else {
            None
        }
    }

    /// Returns the number of keys available in this CrcParams instance.
    #[inline(always)]
    pub fn key_count(&self) -> usize {
        self.keys.key_count()
    }
}

/// The register a kernel starts from. The Rocksoft model gives `init` as the register before
/// reflection, and the reflected kernels hold the register bit-reversed, so a reflected CRC
/// starts from `init` reversed within its width. The catalogued reflected CRC-32s and CRC-64s
/// start from 0 or all ones, which the reversal leaves alone; CRC-16/RIELLO (init 0xB2AA) is
/// one it does not.
pub(crate) fn initial_state(width: u8, init: u64, reflected: bool) -> u64 {
    match (reflected, width) {
        (false, _) => init,
        (true, 16) => (init as u16).reverse_bits() as u64,
        (true, 32) => (init as u32).reverse_bits() as u64,
        (true, _) => init.reverse_bits(),
    }
}

#[cfg(test)]
mod initial_state_tests {
    use super::*;

    /// Custom reflected parameters whose init is not a bit palindrome, against the catalogue
    /// (CRC-16/RIELLO) and a bitwise Rocksoft model (the 32- and 64-bit cases).
    #[test]
    fn a_reflected_custom_init_is_the_catalogued_register() {
        let data = b"123456789";
        let riello = CrcParams::new("CRC-16/RIELLO", 16, 0x1021, 0xb2aa, true, 0, 0x63d0);
        assert_eq!(crate::checksum_with_params(riello, data), 0x63d0);
        let custom32 = CrcParams::new("custom32", 32, 0x04c11db7, 0x12345678, true, 0, 0xf0748bce);
        assert_eq!(crate::checksum_with_params(custom32, data), 0xf0748bce);
        let custom64 = CrcParams::new(
            "custom64",
            64,
            0x42f0e1eba9ea3693,
            0x123456789abcdef0,
            true,
            0,
            0x71730ebfa0af8ee8,
        );
        assert_eq!(
            crate::checksum_with_params(custom64, data),
            0x71730ebfa0af8ee8
        );
        // The forward form starts from init as given.
        assert_eq!(initial_state(32, 0x12345678, false), 0x12345678);
        assert_eq!(initial_state(32, 0x12345678, true), 0x1e6a2c48);
    }
}
