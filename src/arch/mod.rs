// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! This module provides the main entry point for the SIMD CRC calculation.
//!
//! It dispatches to the appropriate architecture-specific implementation

use crate::CrcParams;

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::aes::Aarch64AesOps;

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::aes_sha3::Aarch64AesSha3Ops;

#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
use crate::{
    algorithm,
    structs::{Width16, Width32, Width64},
};

pub mod aarch64;
pub(crate) mod clmul;
pub mod software;
pub(crate) mod sparse;
pub mod x86;
pub mod x86_64;

/// Main entry point that dispatches to the appropriate architecture
///
/// # Safety
/// May use native CPU features
#[inline(always)]
#[cfg(target_arch = "aarch64")]
pub(crate) unsafe fn update(state: u64, bytes: &[u8], params: &CrcParams) -> u64 {
    use crate::feature_detection::{get_arch_ops, ArchOpsInstance};

    match get_arch_ops() {
        ArchOpsInstance::Aarch64AesSha3(ops) => update_aarch64_aes_sha3(state, bytes, params, *ops),
        ArchOpsInstance::Aarch64Aes(ops) => update_aarch64_aes(state, bytes, params, *ops),
        // Detected when the CPU has no PMULL, or forced through `CRC_TURBO_TIER`.
        ArchOpsInstance::SoftwareFallback => crate::arch::software::update(state, bytes, params),
    }
}

/// One aarch64 tier: buffers shorter than 128 bytes inline, longer ones through an out-of-line
/// fold built with the tier's own features. Kept apart, a short buffer does not save and
/// restore the vector registers the folds use: inlined with them, CRC-16/ARC at 16 bytes ran
/// at 0.91x to 0.92x of 1.11.0's speed on Graviton3 and Graviton4. The fold has to carry the
/// tier's features itself, as the SHA3 tier's vector operations do not inline into a function
/// built for AES alone, which ran 3.5x to 7.5x slower there from 128 bytes.
macro_rules! aarch64_tier {
    ($update:ident, $fold:ident, $features:literal, $ops:ty) => {
        #[inline]
        #[cfg(target_arch = "aarch64")]
        #[target_feature(enable = $features)]
        unsafe fn $update(state: u64, bytes: &[u8], params: &CrcParams, ops: $ops) -> u64 {
            if bytes.len() >= 128 {
                return $fold(state, bytes, params, ops);
            }
            match params.width {
                64 => algorithm::update_short::<_, Width64>(state, bytes, params, &ops),
                32 => {
                    algorithm::update_short::<_, Width32>(state as u32, bytes, params, &ops) as u64
                }
                16 => {
                    algorithm::update_short::<_, Width16>(state as u16, bytes, params, &ops) as u64
                }
                _ => panic!("Unsupported CRC width: {}", params.width),
            }
        }

        #[inline(never)]
        #[cfg(target_arch = "aarch64")]
        #[target_feature(enable = $features)]
        unsafe fn $fold(state: u64, bytes: &[u8], params: &CrcParams, ops: $ops) -> u64 {
            // Each arm is compiled with the bit order known, as it was inlined into a caller
            // naming a constant algorithm; tested in the fold instead, it cost up to 14% from
            // 128 bytes to 4 KiB on Graviton 2, 3 and 4.
            macro_rules! by_width {
                () => {
                    match params.width {
                        64 => algorithm::update::<_, Width64>(state, bytes, params, &ops),
                        32 => algorithm::update::<_, Width32>(state as u32, bytes, params, &ops)
                            as u64,
                        16 => algorithm::update::<_, Width16>(state as u16, bytes, params, &ops)
                            as u64,
                        _ => panic!("Unsupported CRC width: {}", params.width),
                    }
                };
            }
            if params.refin {
                by_width!()
            } else {
                by_width!()
            }
        }
    };
}

aarch64_tier!(update_aarch64_aes, fold_aarch64_aes, "aes", Aarch64AesOps);
aarch64_tier!(
    update_aarch64_aes_sha3,
    fold_aarch64_aes_sha3,
    "aes,sha3",
    Aarch64AesSha3Ops
);

/// The x86 tiers' body: every width, each compiled twice, once per bit order, so that the
/// fold inlined into each arm has the order as a constant. Tested inside the fold instead,
/// every 16-byte step branched on it and rebuilt its key pair.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
macro_rules! x86_by_order {
    ($state:expr, $bytes:expr, $params:expr, $ops:expr) => {{
        let (state, bytes, params, ops) = ($state, $bytes, $params, $ops);
        macro_rules! by_width {
            () => {
                match params.width {
                    64 => algorithm::update::<_, Width64>(state, bytes, params, &ops),
                    32 => algorithm::update::<_, Width32>(state as u32, bytes, params, &ops) as u64,
                    16 => algorithm::update::<_, Width16>(state as u16, bytes, params, &ops) as u64,
                    _ => panic!("Unsupported CRC width: {}", params.width),
                }
            };
        }
        if params.refin {
            by_width!()
        } else {
            by_width!()
        }
    }};
}

/// Main entry point for x86/x86_64
///
/// # Safety
/// May use native CPU features
#[inline(always)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) unsafe fn update(state: u64, bytes: &[u8], params: &CrcParams) -> u64 {
    use crate::feature_detection::{get_arch_ops, ArchOpsInstance};

    match get_arch_ops() {
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx512Vpclmulqdq(ops) => {
            update_x86_64_avx512_vpclmulqdq(state, bytes, params, *ops)
        }
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx512Pclmulqdq(ops) => {
            update_x86_64_avx512_pclmulqdq(state, bytes, params, *ops)
        }
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx2Vpclmulqdq(ops) => {
            update_x86_64_avx2_vpclmulqdq(state, bytes, params, *ops)
        }
        ArchOpsInstance::X86SsePclmulqdq(ops) => {
            update_x86_sse_pclmulqdq(state, bytes, params, *ops)
        }
        ArchOpsInstance::SoftwareFallback => crate::arch::software::update(state, bytes, params),
    }
}

/// One x86 tier, built as [`aarch64_tier!`] is: buffers shorter than 128 bytes inline in the
/// entry point, longer ones through an out-of-line fold with the tier's features, each bit
/// order compiled apart. With the folds inlined into the entry point, every call saved six
/// general registers and set up a 280-byte frame before looking at the length.
macro_rules! x86_tier {
    ($(#[$cfg:meta])* $update:ident, $fold:ident, $iso_hdlc:ident, $iso_hdlc_fold:ident,
     $features:literal, $ops:ty) => {
        $(#[$cfg])*
        #[inline]
        #[target_feature(enable = $features)]
        unsafe fn $update(state: u64, bytes: &[u8], params: &CrcParams, ops: $ops) -> u64 {
            if bytes.len() >= 128 {
                return $fold(state, bytes, params, ops);
            }
            match params.width {
                64 => algorithm::update_short::<_, Width64>(state, bytes, params, &ops),
                32 => {
                    algorithm::update_short::<_, Width32>(state as u32, bytes, params, &ops) as u64
                }
                16 => {
                    algorithm::update_short::<_, Width16>(state as u16, bytes, params, &ops) as u64
                }
                _ => panic!("Unsupported CRC width: {}", params.width),
            }
        }

        $(#[$cfg])*
        #[inline(never)]
        #[target_feature(enable = $features)]
        unsafe fn $fold(state: u64, bytes: &[u8], params: &CrcParams, ops: $ops) -> u64 {
            x86_by_order!(state, bytes, params, ops)
        }

        /// CRC-32/ISO-HDLC with its parameters a constant, so that the width, the bit order
        /// and the keys are folded in rather than read and branched on, and each fold
        /// coefficient is one load rather than two moves into the vector unit and a pack.
        $(#[$cfg])*
        #[inline]
        #[target_feature(enable = $features)]
        unsafe fn $iso_hdlc(state: u32, bytes: &[u8], ops: $ops) -> u32 {
            if bytes.len() >= 128 {
                return $iso_hdlc_fold(state, bytes, ops);
            }
            algorithm::update_short::<_, Width32>(
                state,
                bytes,
                &crate::crc32::consts::CRC32_ISO_HDLC,
                &ops,
            )
        }

        $(#[$cfg])*
        #[inline(never)]
        #[target_feature(enable = $features)]
        unsafe fn $iso_hdlc_fold(state: u32, bytes: &[u8], ops: $ops) -> u32 {
            algorithm::update::<_, Width32>(
                state,
                bytes,
                &crate::crc32::consts::CRC32_ISO_HDLC,
                &ops,
            )
        }
    };
}

/// [`update`] for CRC-32/ISO-HDLC.
///
/// x86 has no CRC32 instruction for this polynomial, so it folds like any other algorithm;
/// through [`update`] each call reads the width, bit order and keys from the parameters and
/// branches on them, which a short buffer pays for in full.
///
/// # Safety
///
/// The CPU must have the features of the tier that detection selected.
#[inline(always)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) unsafe fn update_iso_hdlc(state: u32, bytes: &[u8]) -> u32 {
    use crate::feature_detection::{get_arch_ops, ArchOpsInstance};

    match get_arch_ops() {
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx512Vpclmulqdq(ops) => {
            iso_hdlc_x86_64_avx512_vpclmulqdq(state, bytes, *ops)
        }
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx512Pclmulqdq(ops) => {
            iso_hdlc_x86_64_avx512_pclmulqdq(state, bytes, *ops)
        }
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx2Vpclmulqdq(ops) => {
            iso_hdlc_x86_64_avx2_vpclmulqdq(state, bytes, *ops)
        }
        ArchOpsInstance::X86SsePclmulqdq(ops) => iso_hdlc_x86_sse_pclmulqdq(state, bytes, *ops),
        ArchOpsInstance::SoftwareFallback => crate::arch::software::update(
            state as u64,
            bytes,
            &crate::crc32::consts::CRC32_ISO_HDLC,
        ) as u32,
    }
}

x86_tier!(
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    update_x86_sse_pclmulqdq,
    fold_x86_sse_pclmulqdq,
    iso_hdlc_x86_sse_pclmulqdq,
    iso_hdlc_fold_x86_sse_pclmulqdq,
    "sse4.1,pclmulqdq",
    crate::arch::x86::sse::X86SsePclmulqdqOps
);
x86_tier!(
    #[cfg(target_arch = "x86_64")]
    update_x86_64_avx512_pclmulqdq,
    fold_x86_64_avx512_pclmulqdq,
    iso_hdlc_x86_64_avx512_pclmulqdq,
    iso_hdlc_fold_x86_64_avx512_pclmulqdq,
    "avx512vl,pclmulqdq",
    crate::arch::x86_64::avx512::X86_64Avx512PclmulqdqOps
);
x86_tier!(
    #[cfg(target_arch = "x86_64")]
    update_x86_64_avx512_vpclmulqdq,
    fold_x86_64_avx512_vpclmulqdq,
    iso_hdlc_x86_64_avx512_vpclmulqdq,
    iso_hdlc_fold_x86_64_avx512_vpclmulqdq,
    "avx512vl,vpclmulqdq",
    crate::arch::x86_64::avx512_vpclmulqdq::X86_64Avx512VpclmulqdqOps
);
x86_tier!(
    #[cfg(target_arch = "x86_64")]
    update_x86_64_avx2_vpclmulqdq,
    fold_x86_64_avx2_vpclmulqdq,
    iso_hdlc_x86_64_avx2_vpclmulqdq,
    iso_hdlc_fold_x86_64_avx2_vpclmulqdq,
    "avx2,vpclmulqdq,sse4.1,pclmulqdq",
    crate::arch::x86_64::avx2_vpclmulqdq::X86_64Avx2VpclmulqdqOps
);

#[inline(always)]
#[cfg(all(
    not(target_arch = "x86"),
    not(target_arch = "x86_64"),
    not(target_arch = "aarch64")
))]
pub(crate) unsafe fn update(state: u64, bytes: &[u8], params: &CrcParams) -> u64 {
    crate::arch::software::update(state, bytes, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc32::consts::CRC32_BZIP2;
    use crate::crc64::consts::CRC64_NVME;
    use crate::test::consts::{TEST_256_BYTES_STRING, TEST_ALL_CONFIGS, TEST_CHECK_STRING};
    use crate::test::create_aligned_data;
    use crate::test::enums::AnyCrcTestConfig;
    use rand::{rng, RngExt};

    #[test]
    fn test_check_value() {
        for config in TEST_ALL_CONFIGS {
            // direct update() call, which needs XOROUT applied
            let actual = unsafe {
                update(
                    config.get_init_algorithm(),
                    TEST_CHECK_STRING,
                    config.get_params(),
                ) ^ config.get_xorout()
            };

            assert_eq!(
                actual,
                config.get_check(),
                "Mismatch CRC, {}, expected {:#x}, got {:#x}",
                config.get_name(),
                config.get_check(),
                actual
            );
        }
    }

    #[test]
    fn test_256_string() {
        for config in TEST_ALL_CONFIGS {
            let actual = unsafe {
                update(
                    config.get_init_algorithm(),
                    &create_aligned_data(TEST_256_BYTES_STRING),
                    config.get_params(),
                ) ^ config.get_xorout()
            };

            assert_eq!(
                actual,
                config.checksum_with_reference(TEST_256_BYTES_STRING),
                "Mismatch CRC, {}, expected {:#x}, got {:#x}",
                config.get_name(),
                config.get_check(),
                actual
            );
        }
    }

    #[test]
    fn test_512_string() {
        let test_string = b"12345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234561234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456";

        for config in TEST_ALL_CONFIGS {
            let actual = unsafe {
                update(
                    config.get_init_algorithm(),
                    &create_aligned_data(test_string),
                    config.get_params(),
                ) ^ config.get_xorout()
            };

            assert_eq!(
                actual,
                config.checksum_with_reference(test_string),
                "Mismatch CRC, {}, expected {:#x}, got {:#x}",
                config.get_name(),
                config.get_check(),
                actual
            );
        }
    }

    #[test]
    fn test_1024_string() {
        let test_string = b"1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345612345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234561234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456";

        for config in TEST_ALL_CONFIGS {
            let actual = unsafe {
                update(
                    config.get_init_algorithm(),
                    &create_aligned_data(test_string),
                    config.get_params(),
                ) ^ config.get_xorout()
            };

            assert_eq!(
                actual,
                config.checksum_with_reference(test_string),
                "Mismatch CRC, {}, expected {:#x}, got {:#x}",
                config.get_name(),
                config.get_check(),
                actual
            );
        }
    }

    // CRC-64/NVME is a special flower in that Rust's crc library doesn't support it yet, so we have
    // tested values to check against.
    #[test]
    fn test_crc64_nvme_standard_vectors() {
        static CASES: &[(&[u8], u64)] = &[
            // from our own internal tests, since the Check value in the NVM Express® NVM Command
            // Set Specification (Revision 1.0d, December 2023) is incorrect
            // (Section 5.2.1.3.4, Figure 120, page 83).
            (b"123456789", 0xae8b14860a799888),

            // from the NVM Express® NVM Command Set Specification (Revision 1.0d, December 2023),
            // Section 5.2.1.3.5, Figure 122, page 84.
            // https://nvmexpress.org/wp-content/uploads/NVM-Express-NVM-Command-Set-Specification-1.0d-2023.12.28-Ratified.pdf
            // and the Linux kernel
            // https://github.com/torvalds/linux/blob/f3813f4b287e480b1fcd62ca798d8556644b8278/crypto/testmgr.h#L3685-L3695
            (&[0; 4096], 0x6482d367eb22b64e),
            (&[255; 4096], 0xc0ddba7302eca3ac),

            // custom values
            (TEST_256_BYTES_STRING, 0xabdb9e6c30937916),
            (b"", 0),
            (b"@", 0x2808afa9582aa47),
            (b"1\x97", 0xb4af0ae0feb08e0f),
            (b"M\"\xdf", 0x85d7cd041a2a8a5d),
            (b"l\xcd\x13\xd7", 0x1860820ea79b0fa3),
            (&[0; 32], 0xcf3473434d4ecf3b),
            (&[255; 32], 0xa0a06974c34d63c4),
            (b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0A\x0B\x0C\x0D\x0E\x0F\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1A\x1B\x1C\x1D\x1E\x1F", 0xb9d9d4a8492cbd7f),
            (&[0; 1024], 0x691bb2b09be5498a),
            (b"hello, world!", 0xf8046e40c403f1d0),
        ];

        for (input, expected) in CASES {
            unsafe {
                let actual = update(CRC64_NVME.init, input, &CRC64_NVME) ^ CRC64_NVME.xorout;

                assert_eq!(
                    actual, *expected,
                    "Mismatch CRC, expected {:#x}, got {:#x}, input: {:?}",
                    expected, actual, input
                );
            }
        }
    }

    /// Test the "crc32" variant used in PHP's hash() function, which is different from the
    /// crc32() function. It's really just CRC-32/BZIP2 with the output byte-reversed to little
    /// endian.
    ///
    /// https://www.php.net/manual/en/function.hash-file.php#104836
    #[test]
    fn test_crc32_php_standard_vectors() {
        static CASES: &[(&[u8], u64)] = &[
            (b"123456789", 0x181989fc),
            (&[0; 4096], 0xe3380088),
            (&[255; 4096], 0x8f2ae650),
            (b"hello, world!", 0x5eacce7),
        ];

        for (input, expected) in CASES {
            let bzip2_crc = unsafe {
                (update(CRC32_BZIP2.init, input, &CRC32_BZIP2) ^ CRC32_BZIP2.xorout) as u32
            };

            // PHP reverses the byte order of the CRC for some reason
            let actual = bzip2_crc.swap_bytes();

            assert_eq!(
                actual, *expected as u32,
                "Mismatch CRC, expected {:#x}, got {:#x}, input: {:?}",
                expected, actual, input
            );
        }
    }

    /// Test CRC-16/IBM-SDLC check value (reflected variant)
    #[test]
    fn test_crc16_ibm_sdlc_check_value() {
        use crate::crc16::consts::CRC16_IBM_SDLC;

        let actual = unsafe {
            update(CRC16_IBM_SDLC.init, TEST_CHECK_STRING, &CRC16_IBM_SDLC) ^ CRC16_IBM_SDLC.xorout
        };

        assert_eq!(
            actual, 0x906E,
            "CRC-16/IBM-SDLC check value mismatch: expected 0x906E, got {:#x}",
            actual
        );
    }

    /// Test CRC-16/T10-DIF check value (forward/non-reflected variant)
    #[test]
    fn test_crc16_t10_dif_check_value() {
        use crate::crc16::consts::CRC16_T10_DIF;

        let actual = unsafe {
            update(CRC16_T10_DIF.init, TEST_CHECK_STRING, &CRC16_T10_DIF) ^ CRC16_T10_DIF.xorout
        };

        assert_eq!(
            actual, 0xD0DB,
            "CRC-16/T10-DIF check value mismatch: expected 0xD0DB, got {:#x}",
            actual
        );
    }

    /// Skipping for Miri runs due to time constraints, underlying code already covered by other
    /// tests.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_small_lengths_all() {
        // Test each CRC variant
        for config in TEST_ALL_CONFIGS {
            // Test each length from 0 to 255
            for len in 0..=255 {
                test_length(len, config);
            }
        }
    }

    /// Skipping for Miri runs due to time constraints, underlying code already covered by other
    /// tests.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_medium_lengths() {
        // Test each CRC variant
        for config in TEST_ALL_CONFIGS {
            // Test each length from 256 to 1024, which should fold and include handling remainders
            for len in 256..=1024 {
                test_length(len, config);
            }
        }
    }

    /// Skipping for Miri runs due to time constraints, underlying code already covered by other
    /// tests.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_large_lengths() {
        // Test each CRC variant
        for config in TEST_ALL_CONFIGS {
            // Test ~1 MiB just before, at, and just after the folding boundaries
            for len in 1048575..=1048577 {
                test_length(len, config);
            }
        }
    }

    /// Checks one forced x86 tier against the table-driven software implementation: every
    /// algorithm, both bit orders, across the short paths, the fold loop, the stream collapse
    /// and the unaligned head and tail, from the init state and a carried state.
    #[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "std"))]
    fn check_x86_tier_matches_software(tier: impl Fn(u64, &[u8], &CrcParams) -> u64) {
        let mut data = vec![0u8; 8192 + 64];
        rng().fill(&mut data[..]);

        let mut lengths: Vec<usize> = (0..=64).collect();
        lengths.extend(120..=400);
        lengths.extend([511, 512, 513, 1023, 1024, 1025, 4095, 4096, 4097, 8192]);

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let init = config.get_init_algorithm();
            let carried = crate::arch::software::update(init, &data[8192..8192 + 7], params);

            for offset in [0usize, 1, 15, 33] {
                for &len in &lengths {
                    let input = &data[offset..offset + len];
                    for state in [init, carried] {
                        let expected = crate::arch::software::update(state, input, params);
                        let actual = tier(state, input, params);

                        assert_eq!(
                            actual,
                            expected,
                            "{} offset {} len {} state {:#x}",
                            config.get_name(),
                            offset,
                            len,
                            state
                        );
                    }
                }
            }
        }
    }

    /// Each x86 tier, forced on any CPU that has its features, including those where dispatch
    /// prefers a wider tier.
    macro_rules! x86_tier_test {
        ($(#[$cfg:meta])* $test:ident, $update:ident, $iso_hdlc:ident, $ops:expr,
         [$($feature:tt),+]) => {
            #[test]
            #[cfg(feature = "std")]
            $(#[$cfg])*
            #[cfg_attr(miri, ignore)]
            fn $test() {
                if !($(is_x86_feature_detected!($feature))&&+) {
                    eprintln!("skipping {}: the CPU lacks the tier's features", stringify!($test));
                    return;
                }
                let ops = $ops;
                check_x86_tier_matches_software(|state, bytes, params| unsafe {
                    $update(state, bytes, params, ops)
                });
                check_iso_hdlc_matches_software(|state, bytes| unsafe {
                    $iso_hdlc(state, bytes, ops)
                });
            }
        };
    }

    /// A tier's CRC-32/ISO-HDLC entry against the software path, at every length below the
    /// fold and the fold's boundaries above it, from four starts, from the init state and a
    /// carried state.
    #[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "std"))]
    fn check_iso_hdlc_matches_software(tier: impl Fn(u32, &[u8]) -> u32) {
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let mut data = vec![0u8; 8192 + 64];
        rng().fill(&mut data[..]);
        let init = params.init_algorithm;
        let carried = crate::arch::software::update(init, &data[8192..8192 + 7], params);

        let mut lengths: Vec<usize> = (0..=400).collect();
        lengths.extend([511, 512, 513, 1023, 1024, 1025, 4095, 4096, 4097, 8192]);
        for offset in [0usize, 1, 15, 33] {
            for &len in &lengths {
                let input = &data[offset..offset + len];
                for state in [init, carried] {
                    let expected = crate::arch::software::update(state, input, params);
                    let actual = tier(state as u32, input) as u64;
                    assert_eq!(
                        actual, expected,
                        "offset {offset} len {len} state {state:#x}"
                    );
                }
            }
        }
    }

    x86_tier_test!(
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        test_sse_pclmulqdq_tier_matches_software,
        update_x86_sse_pclmulqdq,
        iso_hdlc_x86_sse_pclmulqdq,
        crate::arch::x86::sse::X86SsePclmulqdqOps,
        ["sse4.1", "pclmulqdq"]
    );
    x86_tier_test!(
        #[cfg(target_arch = "x86_64")]
        test_avx2_vpclmulqdq_tier_matches_software,
        update_x86_64_avx2_vpclmulqdq,
        iso_hdlc_x86_64_avx2_vpclmulqdq,
        crate::arch::x86_64::avx2_vpclmulqdq::X86_64Avx2VpclmulqdqOps::default(),
        ["avx2", "vpclmulqdq", "sse4.1", "pclmulqdq"]
    );
    x86_tier_test!(
        #[cfg(target_arch = "x86_64")]
        test_avx512_pclmulqdq_tier_matches_software,
        update_x86_64_avx512_pclmulqdq,
        iso_hdlc_x86_64_avx512_pclmulqdq,
        crate::arch::x86_64::avx512::X86_64Avx512PclmulqdqOps::default(),
        ["avx512vl", "pclmulqdq"]
    );
    x86_tier_test!(
        #[cfg(target_arch = "x86_64")]
        test_avx512_vpclmulqdq_tier_matches_software,
        update_x86_64_avx512_vpclmulqdq,
        iso_hdlc_x86_64_avx512_vpclmulqdq,
        crate::arch::x86_64::avx512_vpclmulqdq::X86_64Avx512VpclmulqdqOps::default(),
        ["avx512vl", "vpclmulqdq"]
    );

    /// Key tables built before the final reduction's 64-bit Barrett quotient hold 0 in its slot,
    /// `keys[0]`; for them the reduction keeps its four-multiply form. Checks every algorithm
    /// with that slot cleared against the table-driven software implementation, across the
    /// short paths, the folds and the unaligned tails, from the init state and a carried state.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_key_tables_without_barrett_quotient_match_software() {
        let mut data = vec![0u8; 4096 + 64];
        rng().fill(&mut data[..]);

        let mut lengths: Vec<usize> = (0..=300).collect();
        lengths.extend([511, 512, 513, 1023, 1024, 1025, 4095, 4096]);

        for config in TEST_ALL_CONFIGS {
            let mut params = *config.get_params();
            let mut keys = *params.keys.fold_keys();
            keys[0] = 0;
            params.keys = crate::CrcKeysStorage::from_keys_fold_256(keys);
            let init = config.get_init_algorithm();
            let carried = crate::arch::software::update(init, &data[4096..4096 + 7], &params);

            for offset in [0usize, 3] {
                for &len in &lengths {
                    let input = &data[offset..offset + len];
                    for state in [init, carried] {
                        assert_eq!(
                            unsafe { update(state, input, &params) },
                            crate::arch::software::update(state, input, &params),
                            "{} offset {} len {} state {:#x}",
                            config.get_name(),
                            offset,
                            len,
                            state
                        );
                    }
                }
            }
        }
    }

    /// Forces each aarch64 tier, the SHA3 one on any CPU that has it whatever dispatch picks, and
    /// checks every algorithm, both bit orders, against the table-driven software implementation
    /// across the 128-byte fold at every chunk count from two to nine and the unaligned head and
    /// tail, from the init state and a carried state.
    #[test]
    #[cfg(all(target_arch = "aarch64", feature = "std"))]
    #[cfg_attr(miri, ignore)]
    fn test_aarch64_tiers_match_software() {
        if !std::arch::is_aarch64_feature_detected!("aes") {
            eprintln!("skipping test_aarch64_tiers_match_software: no AES");
            return;
        }
        let sha3 = std::arch::is_aarch64_feature_detected!("sha3");

        let mut data = vec![0u8; 8192 + 64];
        rng().fill(&mut data[..]);

        let mut lengths: Vec<usize> = (0..=64).collect();
        lengths.extend(120..=300);
        for chunks in 2..=9 {
            lengths.extend([
                128 * chunks - 1,
                128 * chunks,
                128 * chunks + 1,
                128 * chunks + 77,
            ]);
        }
        lengths.extend([2048, 2049, 4095, 4096, 4097, 8192]);

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let init = config.get_init_algorithm();
            let carried = crate::arch::software::update(init, &data[8192..8192 + 7], params);

            for offset in [0usize, 1, 15, 33] {
                for &len in &lengths {
                    let input = &data[offset..offset + len];
                    for state in [init, carried] {
                        let expected = crate::arch::software::update(state, input, params);
                        let aes =
                            unsafe { update_aarch64_aes(state, input, params, Aarch64AesOps) };
                        assert_eq!(
                            aes,
                            expected,
                            "aes {} offset {} len {} state {:#x}",
                            config.get_name(),
                            offset,
                            len,
                            state
                        );
                        if sha3 {
                            let sha3 = unsafe {
                                update_aarch64_aes_sha3(
                                    state,
                                    input,
                                    params,
                                    Aarch64AesSha3Ops::new(),
                                )
                            };
                            assert_eq!(
                                sha3,
                                expected,
                                "aes+sha3 {} offset {} len {} state {:#x}",
                                config.get_name(),
                                offset,
                                len,
                                state
                            );
                        }
                    }
                }
            }
        }
    }

    fn test_length(length: usize, config: &AnyCrcTestConfig) {
        let mut data = vec![0u8; length];
        rng().fill(&mut data[..]);

        // Calculate expected CRC using the reference implementation
        let expected = config.checksum_with_reference(&data);

        // direct update() call, which needs XOROUT applied
        let actual = unsafe {
            update(config.get_init_algorithm(), &data, config.get_params()) ^ config.get_xorout()
        };

        assert_eq!(
            actual,
            expected,
            "\nFailed for {} with length {}\nGot: {:016x}\nExpected: {:016x}",
            config.get_name(),
            length,
            actual,
            expected
        );
    }
}
