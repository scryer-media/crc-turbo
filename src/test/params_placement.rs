// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.

//! Where the built-in parameter sets sit when `checksum` hands them to the kernels.
//!
//! A parameter set that spans a page boundary makes every key pair the fold reads from across it
//! a split load. With the statics packed 8-byte aligned by the linker, which one spans a page
//! changed from build to build: on an Apple M5 Max it was CRC-64/MS in one, at 3.7 GB/s for 64
//! bytes against 17 for CRC-64/XZ. The test pins the construction that rules it out, and checks
//! addresses, not time.

#![cfg(all(
    feature = "std",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]

use crate::test::consts::TEST_ALL_CONFIGS;
use crate::test::PARAMS_SEEN;
use crate::{checksum, CrcAlgorithm, CrcParams};

/// The smallest page size of any target the crate builds for.
const MIN_PAGE: usize = 4096;

#[test]
fn test_checksum_params_never_span_a_page() {
    let data = [0x5au8; 64];
    let size = size_of::<CrcParams>();
    for config in TEST_ALL_CONFIGS {
        let algorithm = config.get_algorithm();
        PARAMS_SEEN.with(|seen| seen.set(0));
        checksum(algorithm, &data);
        let addr = PARAMS_SEEN.with(|seen| seen.get());
        if addr == 0 {
            // Taken by a dedicated kernel that reads no parameter set.
            assert!(
                matches!(
                    algorithm,
                    CrcAlgorithm::Crc32Iscsi | CrcAlgorithm::Crc32IsoHdlc
                ),
                "{algorithm:?} never reached the calculator"
            );
            continue;
        }
        assert_eq!(
            addr / MIN_PAGE,
            (addr + size - 1) / MIN_PAGE,
            "{algorithm:?}: its parameters at {addr:#x} span a page boundary"
        );
        assert_eq!(
            addr % size.next_power_of_two(),
            0,
            "{algorithm:?}: its parameters at {addr:#x} are not aligned to their own size, so \
             another build may put them across a page"
        );
    }
}
