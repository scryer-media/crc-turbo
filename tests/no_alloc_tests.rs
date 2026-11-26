//! Custom parameters in a build without `alloc`, against the CRC catalogue's check values.
//!
//! On a target with no accelerated tier, the build without `alloc` computes custom parameters
//! through its allocation-free software path, which these exercise:
//! `cargo test --no-default-features --test no_alloc_tests`. On other targets, and in builds
//! with `alloc`, they check the same values through whichever path dispatch picks.

use crc_turbo::{checksum_with_params, CrcParams, Digest};

/// (width, poly, init, reflected, xorout, check): CRC-16/IBM-SDLC, CRC-16/UMTS,
/// CRC-16/RIELLO, CRC-32/ISO-HDLC, CRC-32/BZIP2, CRC-64/XZ and CRC-64/ECMA-182.
const CASES: [(u8, u64, u64, bool, u64, u64); 7] = [
    (16, 0x1021, 0xffff, true, 0xffff, 0x906e),
    (16, 0x8005, 0x0000, false, 0x0000, 0xfee8),
    (16, 0x1021, 0xb2aa, true, 0x0000, 0x63d0),
    (32, 0x04c1_1db7, 0xffff_ffff, true, 0xffff_ffff, 0xcbf4_3926),
    (
        32,
        0x04c1_1db7,
        0xffff_ffff,
        false,
        0xffff_ffff,
        0xfc89_1918,
    ),
    (
        64,
        0x42f0_e1eb_a9ea_3693,
        u64::MAX,
        true,
        u64::MAX,
        0x995d_c9bb_df19_39fa,
    ),
    (
        64,
        0x42f0_e1eb_a9ea_3693,
        0,
        false,
        0,
        0x6c40_df5f_0b49_7347,
    ),
];

#[test]
fn custom_parameters_give_the_check_values() {
    for (width, poly, init, reflected, xorout, check) in CASES {
        let params = CrcParams::new("custom", width, poly, init, reflected, xorout, check);
        assert_eq!(
            checksum_with_params(params, b"123456789"),
            check,
            "width {width} poly {poly:#x} reflected {reflected}"
        );
    }
}

#[test]
fn custom_parameters_carry_state_across_updates() {
    for (width, poly, init, reflected, xorout, check) in CASES {
        let params = CrcParams::new("custom", width, poly, init, reflected, xorout, check);
        let mut digest = Digest::new_with_params(params);
        digest.update(b"1234");
        digest.update(b"");
        digest.update(b"56789");
        assert_eq!(
            digest.finalize(),
            check,
            "width {width} poly {poly:#x} reflected {reflected}"
        );
    }
}
