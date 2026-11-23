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

/// One more than the longest buffer x86's CRC-32/ISCSI takes with the CRC32 instruction alone,
/// [`x86::IscsiPlan::direct_max`]: 0 until the plan is made, and 0 for good where it has no
/// kernel. A buffer shorter than it is a relaxed load and a compare ahead of the CRC32
/// instructions; nothing else of the plan is read for it. A nonzero value is only ever stored
/// after detecting SSE4.2, and every thread that stores one stores the same.
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
static ISCSI_DIRECT_END: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// x86's CRC-32/ISCSI plan, made once, or `None` where the CPU has no kernel.
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
static ISCSI_PLAN: std::sync::OnceLock<Option<x86::IscsiPlan>> = std::sync::OnceLock::new();

/// CRC-32/ISCSI through this CPU's plan: the CRC32 instruction alone for a short buffer, the
/// fusion kernel after that, or the folding path where the plan has it take that.
///
/// It is never `None` on x86. What the caller inlines is the short arm, written so that it
/// inlines, and the call to the kernel; making the plan and the folding path are behind
/// [`crc32_iscsi_planned`]. So the call site carries no fold of its own, keeps nothing live
/// across a call, and saves no registers before looking at the length.
#[inline(always)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
pub(crate) fn crc32_iscsi(state: u32, data: &[u8]) -> Option<u32> {
    use core::sync::atomic::Ordering;

    if data.len() < ISCSI_DIRECT_END.load(Ordering::Relaxed) {
        // SAFETY: the limit is nonzero only once a kernel was selected, after detecting SSE4.2
        return Some(unsafe { x86::crc32_iscsi_direct(state, data) });
    }

    if let Some(Some(plan)) = ISCSI_PLAN.get() {
        if data.len() < plan.fold_min || data.len() >= plan.fold_end {
            // SAFETY: the kernel was selected for the tier this CPU runs, which has the
            // carry-less multiply it needs, after detecting the CRC32 instructions (and AVX2 for
            // its AVX2 build)
            return Some(unsafe { (plan.kernel)(state, data) });
        }
    }

    Some(crc32_iscsi_planned(state, data))
}

/// [`crc32_iscsi`] for the first call, which makes the plan, and for the lengths the plan folds.
#[inline(never)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
fn crc32_iscsi_planned(state: u32, data: &[u8]) -> u32 {
    use core::sync::atomic::Ordering;

    let plan = ISCSI_PLAN.get_or_init(|| {
        let tier = crate::feature_detection::get_arch_ops().get_tier();
        let kernel = x86::select_iscsi_kernel(
            tier,
            std::arch::is_x86_feature_detected!("sse4.2"),
            std::arch::is_x86_feature_detected!("avx2"),
        )?;
        let plan = x86::iscsi_plan(kernel, tier, x86::CpuId::detect());
        ISCSI_DIRECT_END.store(plan.direct_max + 1, Ordering::Relaxed);

        Some(plan)
    });

    if let Some(plan) = plan {
        if data.len() <= plan.direct_max {
            // SAFETY: a kernel is only selected after detecting SSE4.2
            return unsafe { x86::crc32_iscsi_small_unaligned(state, data) };
        }

        if data.len() < plan.fold_min || data.len() >= plan.fold_end {
            // SAFETY: as in `crc32_iscsi`
            return unsafe { (plan.kernel)(state, data) };
        }
    }

    // SAFETY: the tier's entry is the one detection selected
    unsafe { crate::arch::update(state as u64, data, &crate::crc32::consts::CRC32_ISCSI) as u32 }
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
