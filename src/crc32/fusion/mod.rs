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
mod x86;

/// A fusion kernel: the CRC state after `data`, from the state before it. `unsafe` because it
/// carries the target features of the tier it was selected for.
#[cfg(all(feature = "std", target_arch = "aarch64"))]
type FusionFn = unsafe fn(u32, &[u8]) -> u32;

/// CRC-32/ISCSI through this CPU's fusion kernel, or `None` when it has to take the folding path.
/// Both AArch64 and x86 have native CRC-32/ISCSI instructions.
#[inline(always)]
#[cfg(all(feature = "std", target_arch = "aarch64"))]
pub(crate) fn crc32_iscsi(state: u32, data: &[u8]) -> Option<u32> {
    static KERNEL: std::sync::OnceLock<Option<FusionFn>> = std::sync::OnceLock::new();

    let kernel = (*KERNEL.get_or_init(|| {
        let tier = crate::feature_detection::get_arch_ops().get_tier();

        aarch64::select_kernel(tier, std::arch::is_aarch64_feature_detected!("crc"))
            .map(aarch64::iscsi_kernel_fn)
    }))?;

    // SAFETY: the kernel was selected for the tier this CPU runs, which has the carry-less
    // multiply it needs, after detecting the CRC32 instructions
    Some(unsafe { kernel(state, data) })
}

/// CRC-32/ISCSI through this CPU's plan: the CRC32 instruction alone for a short buffer, the
/// fusion kernel after that, or `None` where the plan has it take the folding path.
#[inline(always)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
pub(crate) fn crc32_iscsi(state: u32, data: &[u8]) -> Option<u32> {
    static PLAN: std::sync::OnceLock<Option<x86::IscsiPlan>> = std::sync::OnceLock::new();

    let plan = (*PLAN.get_or_init(|| {
        let tier = crate::feature_detection::get_arch_ops().get_tier();
        let kernel = x86::select_iscsi_kernel(
            tier,
            std::arch::is_x86_feature_detected!("sse4.2"),
            std::arch::is_x86_feature_detected!("avx2"),
        )?;

        Some(x86::iscsi_plan(kernel, tier, x86::CpuId::detect()))
    }))?;

    if data.len() <= plan.direct_max {
        // A short buffer is a direct call, ahead of the kernel's pointer.
        // SAFETY: a kernel is only selected after detecting SSE4.2
        return Some(unsafe { x86::crc32_iscsi_small_unaligned(state, data) });
    }

    if data.len() >= plan.fold_min {
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
    static KERNEL: std::sync::OnceLock<Option<FusionFn>> = std::sync::OnceLock::new();

    let kernel = (*KERNEL.get_or_init(|| {
        let tier = crate::feature_detection::get_arch_ops().get_tier();

        aarch64::select_kernel(tier, std::arch::is_aarch64_feature_detected!("crc"))
            .map(aarch64::iso_hdlc_kernel_fn)
    }))?;

    // SAFETY: as for `crc32_iscsi`
    Some(unsafe { kernel(state, data) })
}
