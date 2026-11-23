// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! This module provides support for calculating CRC-32/ISO-HDLC and CRC-32/ISCSI using
//! fusion techniques.
//!
//! <https://www.corsix.org/content/fast-crc32c-4k>
//! <https://www.corsix.org/content/alternative-exposition-crc32_4k_pclmulqdq>
//! <https://dougallj.wordpress.com/2022/05/22/faster-crc32-on-the-apple-m1/>
//! <https://github.com/corsix/fast-crc32/>
//!
//! Which kernel a CPU runs is decided once, from the tier and the CRC32 instructions, and kept
//! as a function pointer: a checksum call loads that pointer and calls it, with no feature
//! detection of its own.

mod aarch64;
mod powers;
mod x86;

/// The aarch64 fusion kernel this CPU runs, for CRC-32/ISCSI and CRC-32/ISO-HDLC alike, or
/// `None` when it has to take the folding path.
///
/// The choice is kept in one byte, read with a relaxed load: 0 before it is made, then 1 for
/// none, 2 for the PMULL kernel and 3 for the EOR3 one. The byte carries the choice itself and
/// publishes nothing else, and every thread that makes it makes the same one. The caller then
/// calls the CRC32 loop or the kernel directly, which a pointer would not let it inline: where
/// the target already has the features, as on Apple's, `checksum` calls the CRC32 loop or the
/// fold itself, with no wrapper between.
#[inline(always)]
#[cfg(all(feature = "std", target_arch = "aarch64"))]
fn aarch64_kernel() -> Option<aarch64::FusionKernel> {
    use core::sync::atomic::{AtomicU8, Ordering};

    use aarch64::FusionKernel;

    static KERNEL: AtomicU8 = AtomicU8::new(0);

    #[cold]
    fn choose() -> Option<FusionKernel> {
        let tier = crate::feature_detection::get_arch_ops().get_tier();
        let kernel = aarch64::select_kernel(tier, std::arch::is_aarch64_feature_detected!("crc"));
        let byte = match kernel {
            None => 1,
            Some(FusionKernel::Pmull) => 2,
            Some(FusionKernel::PmullSha3) => 3,
        };
        KERNEL.store(byte, Ordering::Relaxed);

        kernel
    }

    match KERNEL.load(Ordering::Relaxed) {
        0 => choose(),
        2 => Some(FusionKernel::Pmull),
        3 => Some(FusionKernel::PmullSha3),
        _ => None,
    }
}

/// CRC-32/ISCSI through this CPU's fusion kernel, or `None` when it has to take the folding path.
/// Both AArch64 and x86 have native CRC-32/ISCSI instructions.
#[inline(always)]
#[cfg(all(feature = "std", target_arch = "aarch64"))]
pub(crate) fn crc32_iscsi(state: u32, data: &[u8]) -> Option<u32> {
    let kernel = aarch64_kernel()?;

    // SAFETY: the kernel was selected for the tier this CPU runs
    Some(unsafe { aarch64::crc32_iscsi_with(kernel, state, data) })
}

/// CRC-32/ISCSI through this CPU's plan: the CRC32 instruction alone for a short buffer, the
/// fusion kernel after that, or `None` where the plan has it take the folding path.
#[inline(always)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
pub(crate) fn crc32_iscsi(state: u32, data: &[u8]) -> Option<u32> {
    static PLAN: std::sync::OnceLock<Option<x86::IscsiPlan>> = std::sync::OnceLock::new();

    // Borrowed: the short path reads one field of it and should not copy the rest.
    let plan = PLAN
        .get_or_init(|| {
            let tier = crate::feature_detection::get_arch_ops().get_tier();
            let kernel = x86::select_iscsi_kernel(
                tier,
                std::arch::is_x86_feature_detected!("sse4.2"),
                std::arch::is_x86_feature_detected!("avx2"),
            )?;

            Some(x86::iscsi_plan(kernel, tier, x86::CpuId::detect()))
        })
        .as_ref()?;

    if data.len() <= plan.direct_max {
        // A short buffer is a direct call, ahead of the kernel's pointer.
        // SAFETY: a kernel is only selected after detecting SSE4.2
        return Some(unsafe { x86::crc32_iscsi_small_unaligned(state, data) });
    }

    if data.len() >= plan.fold_min && data.len() < plan.fold_end {
        return None;
    }

    // SAFETY: the kernel was selected for the tier this CPU runs, which has the carry-less
    // multiply it needs, after detecting the CRC32 instructions (and AVX2 for its AVX2 build)
    Some(unsafe { (plan.kernel)(state, data) })
}

/// CRC-32/ISO-HDLC through this CPU's fusion kernel, or `None` when it has to take the folding
/// path. Only AArch64 has native CRC-32/ISO-HDLC instructions.
#[inline(always)]
#[cfg(all(feature = "std", target_arch = "aarch64"))]
pub(crate) fn crc32_iso_hdlc(state: u32, data: &[u8]) -> Option<u32> {
    let kernel = aarch64_kernel()?;

    // SAFETY: the kernel was selected for the tier this CPU runs
    Some(unsafe { aarch64::crc32_iso_hdlc_with(kernel, state, data) })
}
