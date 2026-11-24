// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.

//! How much stack a checksum needs in the build the tests run in.
//!
//! The kernels are built from `#[inline(always)]` functions, and an unoptimized build gives every
//! temporary of every function inlined into a frame its own stack slot. With each width and bit
//! order's whole kernel inlined into one function, that frame was 1.7 MiB on aarch64 and 1.2 MiB
//! on x86_64 (67 KiB and 87 KiB in 1.11.0), which overflows the 1 MiB main thread of a Windows
//! process: a debug build crashed in `checksum`. The test runs every algorithm over the lengths
//! that reach each kernel on a thread with a fixed small stack. It checks a size, not a time.

#![cfg(all(feature = "std", not(target_family = "wasm")))]

use crate::test::consts::TEST_ALL_CONFIGS;
use crate::{checksum, checksum_combine, checksum_zeros, CrcAlgorithm, Digest};

/// The stack the thread gets. Four times what the widest kernel needs unoptimized on x86_64
/// and aarch64, and a quarter of a Windows main thread.
const STACK: usize = 256 * 1024;

/// Every length to 300, then the lengths around each boundary the kernels branch on.
fn lengths() -> Vec<usize> {
    let mut lengths: Vec<usize> = (0..=300).collect();
    for boundary in [384usize, 448, 512, 1024, 2048, 4096, 8192, 16384, 65536] {
        lengths.extend([boundary - 1, boundary, boundary + 1, boundary + 17]);
    }
    lengths.push(100_000);
    lengths
}

fn run(algorithms: &[CrcAlgorithm], data: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    for &algorithm in algorithms {
        for len in lengths() {
            // From an unaligned start, as well as from the allocation's.
            for start in [0usize, 3] {
                let bytes = &data[start..start + len];
                let crc = checksum(algorithm, bytes);
                let mut digest = Digest::new(algorithm);
                let (head, tail) = bytes.split_at(len / 3);
                digest.update(head);
                digest.update(tail);
                assert_eq!(digest.finalize(), crc);
                assert_eq!(
                    checksum_combine(
                        algorithm,
                        checksum(algorithm, head),
                        checksum(algorithm, tail),
                        tail.len() as u64
                    ),
                    crc
                );
                out.push(crc);
            }
            out.push(checksum_zeros(algorithm, len as u64));
        }
    }
    out
}

#[test]
#[cfg_attr(miri, ignore)] // Miri's stack is not the host's, and has no such limit.
fn test_every_algorithm_fits_a_small_stack() {
    let algorithms: Vec<CrcAlgorithm> = TEST_ALL_CONFIGS
        .iter()
        .map(|config| config.get_algorithm())
        .collect();
    let data: Vec<u8> = (0..100_003u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();

    let (algorithms_there, data_there) = (algorithms.clone(), data.clone());
    let there = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || run(&algorithms_there, &data_there))
        .expect("the thread starts")
        .join()
        .expect("every checksum fits the stack");

    assert_eq!(there, run(&algorithms, &data));
}
