// Copyright 2026 the crc-turbo authors. Licensed under MIT or Apache-2.0.

//! `CRC_TURBO_TIER` pins the tier for a whole process, so it is exercised through the CLI tools
//! in child processes rather than in the test process, whose tier is already chosen.

#![cfg(feature = "cli")]

use std::process::Command;

const TIERS: &[&str] = &[
    "aarch64-neon-pmull-sha3",
    "aarch64-neon-pmull",
    "x86_64-avx512-vpclmulqdq",
    "x86_64-avx512-pclmulqdq",
    "x86_64-avx2-vpclmulqdq",
    "x86_64-sse-pclmulqdq",
    "x86-sse-pclmulqdq",
    "software-fallback-tables",
];

fn run(bin: &str, tier: Option<&str>, args: &[&str]) -> String {
    let mut command = Command::new(bin);
    command.args(args).env_remove(crc_turbo::TIER_ENV);
    if let Some(tier) = tier {
        command.env(crc_turbo::TIER_ENV, tier);
    }
    let output = command.output().expect("run the CLI tool");
    assert!(
        output.status.success(),
        "{bin} {args:?} with {tier:?} failed"
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn target(tier: Option<&str>) -> String {
    let report = run(env!("CARGO_BIN_EXE_arch-check"), tier, &[]);
    report
        .lines()
        .find_map(|line| line.split("CRC-64/NVME target: ").nth(1))
        .expect("arch-check reports the CRC-64/NVME target")
        .trim()
        .to_string()
}

#[test]
#[cfg_attr(miri, ignore)] // Miri's isolation forbids spawning processes
fn forcing_a_tier_selects_it_when_the_cpu_has_it_and_is_ignored_otherwise() {
    let detected = target(None);
    assert!(TIERS.contains(&detected.as_str()), "{detected}");

    // software runs everywhere
    assert_eq!(
        target(Some("software-fallback-tables")),
        "software-fallback-tables"
    );
    // a value that names no tier changes nothing
    assert_eq!(target(Some("not-a-tier")), detected);

    for tier in TIERS {
        let selected = target(Some(tier));
        assert!(
            selected == *tier
                || selected == detected
                // the SSE implementation reports one name for both x86 spellings
                || (tier.ends_with("sse-pclmulqdq") && selected.ends_with("sse-pclmulqdq")),
            "forcing {tier} selected {selected} (detected {detected})"
        );
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Miri's isolation forbids spawning processes
fn every_forced_tier_computes_the_check_values() {
    for tier in TIERS {
        for (algorithm, check) in [
            ("CRC-16/ARC", "0xbb3d"),
            ("CRC-16/IBM-3740", "0x29b1"),
            ("CRC-32/ISO-HDLC", "0xcbf43926"),
            ("CRC-32/ISCSI", "0xe3069283"),
            ("CRC-32/BZIP2", "0xfc891918"),
            ("CRC-64/NVME", "0xae8b14860a799888"),
            ("CRC-64/XZ", "0x995dc9bbdf1939fa"),
        ] {
            let output = run(
                env!("CARGO_BIN_EXE_checksum"),
                Some(tier),
                &["-a", algorithm, "-s", "123456789"],
            );
            assert_eq!(output.trim(), check, "{algorithm} forced to {tier}");
        }
    }
}
