// Copyright 2026 the crc-turbo authors. Licensed under MIT or Apache-2.0.

//! The contract the public API keeps on every tier, checked against the `crc` crate's
//! table-driven implementation: every start alignment, no reads outside the input, and a
//! digest split anywhere. Requested in crc-fast#64 by GiorgioRegni.

#![cfg(all(test, feature = "std"))]

use rand::{rng, RngExt};

use crate::test::consts::TEST_ALL_CONFIGS;
use crate::{checksum, Digest};

/// Lengths around every alignment prologue, fold boundary and the fusion kernels' large-buffer
/// forms, with the stretch below 192 bytes covered completely.
fn lengths(max: usize) -> Vec<usize> {
    let mut lengths: Vec<usize> = (0..=192).collect();
    lengths.extend([
        255, 256, 257, 383, 384, 385, 511, 512, 513, 1023, 1024, 1025, 1087, 1088, 1089, 2047,
        2048, 2049, 4095, 4096, 4097,
    ]);
    lengths.retain(|&len| len <= max);
    lengths
}

/// Every built-in algorithm, from every start offset within a cache line, matches the
/// reference; the offset sweep reaches every alignment prologue of every tier.
#[test]
#[cfg_attr(miri, ignore)] // the accelerated tiers use intrinsics Miri does not model
fn every_start_offset_matches_the_reference() {
    let mut data = vec![0u8; 4096 + 64];
    rng().fill(&mut data[..]);

    for config in TEST_ALL_CONFIGS {
        for offset in 0..64 {
            for len in lengths(4096) {
                let input = &data[offset..offset + len];
                assert_eq!(
                    checksum(config.get_algorithm(), input),
                    config.checksum_with_reference(input),
                    "{} offset {offset} len {len}",
                    config.get_name()
                );
            }
        }
    }
}

/// A buffer whose only mapped page sits between two unmapped guard pages, so a read before
/// its start or past its end faults.
#[cfg(unix)]
struct GuardedPage {
    base: *mut u8,
    page: usize,
}

#[cfg(unix)]
impl GuardedPage {
    fn new() -> Self {
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
            .expect("a positive page size");
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                3 * page,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(base, libc::MAP_FAILED, "mmap three pages");
        let base = base.cast::<u8>();
        for guard in [0, 2 * page] {
            let protected =
                unsafe { libc::mprotect(base.add(guard).cast(), page, libc::PROT_NONE) };
            assert_eq!(protected, 0, "protect the guard page");
        }
        Self { base, page }
    }

    /// The mapped page.
    fn page(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.base.add(self.page), self.page) }
    }
}

#[cfg(unix)]
impl Drop for GuardedPage {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base.cast(), 3 * self.page) };
    }
}

/// No tier reads outside its input: inputs that end exactly at an unmapped page, and inputs
/// that start exactly after one, checksum without faulting, at every length up to a page.
#[test]
#[cfg(unix)]
#[cfg_attr(miri, ignore)] // mmap and the accelerated tiers are outside Miri
fn no_tier_reads_outside_its_input() {
    let mut guarded = GuardedPage::new();
    let page = guarded.page();
    rng().fill(page);
    let size = page.len();

    let mut lengths = lengths(size);
    lengths.extend((size - 64)..=size);

    for config in TEST_ALL_CONFIGS {
        for &len in &lengths {
            let ending_at_the_guard = &page[size - len..];
            assert_eq!(
                checksum(config.get_algorithm(), ending_at_the_guard),
                config.checksum_with_reference(ending_at_the_guard),
                "{} len {len} ending at the guard page",
                config.get_name()
            );
            let starting_after_the_guard = &page[..len];
            assert_eq!(
                checksum(config.get_algorithm(), starting_after_the_guard),
                config.checksum_with_reference(starting_after_the_guard),
                "{} len {len} starting after the guard page",
                config.get_name()
            );
        }
    }
}

/// A digest fed its input in two parts, split at every point, matches the one-shot checksum:
/// the carried state crosses every alignment prologue and every kernel boundary.
#[test]
#[cfg_attr(miri, ignore)] // the accelerated tiers use intrinsics Miri does not model
fn a_digest_split_at_every_point_matches_the_reference() {
    let mut data = vec![0u8; 3000];
    rng().fill(&mut data[..]);

    for config in TEST_ALL_CONFIGS {
        let expected = config.checksum_with_reference(&data);
        for split in 0..=data.len() {
            let mut digest = Digest::new(config.get_algorithm());
            digest.update(&data[..split]);
            digest.update(&data[split..]);
            assert_eq!(
                digest.finalize(),
                expected,
                "{} split at {split}",
                config.get_name()
            );
        }
    }
}
