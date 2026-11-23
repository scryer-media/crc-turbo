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

/// The aarch64 fusion kernel's choice, kept in one byte and read with a relaxed load: 0 before
/// it is made, then 1 for none, 2 for the PMULL kernel and 3 for the EOR3 one. The byte carries
/// the choice itself and publishes nothing else, and every thread that makes it makes the same
/// one.
#[cfg(all(feature = "std", target_arch = "aarch64"))]
static AARCH64_KERNEL: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Makes the aarch64 kernel's choice and stores it in [`AARCH64_KERNEL`].
#[cold]
#[cfg(all(feature = "std", target_arch = "aarch64"))]
fn choose_aarch64_kernel() {
    use aarch64::FusionKernel;

    let tier = crate::feature_detection::get_arch_ops().get_tier();
    let kernel = aarch64::select_kernel(tier, std::arch::is_aarch64_feature_detected!("crc"));
    let byte = match kernel {
        None => 1,
        Some(FusionKernel::Pmull) => 2,
        Some(FusionKernel::PmullSha3) => 3,
    };
    AARCH64_KERNEL.store(byte, core::sync::atomic::Ordering::Relaxed);
}

/// The aarch64 entry for one CRC-32, with `XOROUT` applied: the kernel the CPU's choice names,
/// called directly, or [`$slow`] before the choice is made and where it is none. Every arm is
/// the last thing the caller does, so a call naming the algorithm is a jump to the kernel, with
/// no frame set up around it: the kernels apply `XOROUT` themselves, as fast-crc32's apply their
/// complement, and the choice and the folding path sit out of line behind a jump of their own.
macro_rules! aarch64_entry {
    ($(#[$doc:meta])* $name:ident, $slow:ident, $with:ident, $params:path) => {
        $(#[$doc])*
        #[inline(always)]
        #[cfg(all(feature = "std", target_arch = "aarch64"))]
        pub(crate) fn $name<const XOROUT: u32>(state: u32, data: &[u8]) -> Option<u32> {
            use core::sync::atomic::Ordering;

            use aarch64::FusionKernel;

            // SAFETY: the byte names a kernel only once one was selected for the tier this CPU
            // runs
            Some(match AARCH64_KERNEL.load(Ordering::Relaxed) {
                3 => unsafe { aarch64::$with(FusionKernel::PmullSha3, state, data, XOROUT) },
                2 => unsafe { aarch64::$with(FusionKernel::Pmull, state, data, XOROUT) },
                _ => $slow::<XOROUT>(state, data),
            })
        }

        /// [`$name`] before the choice is made, which makes it and goes back, and where the
        /// choice is none, which takes the folding path.
        #[inline(never)]
        #[cfg(all(feature = "std", target_arch = "aarch64"))]
        fn $slow<const XOROUT: u32>(state: u32, data: &[u8]) -> u32 {
            use core::sync::atomic::Ordering;

            if AARCH64_KERNEL.load(Ordering::Relaxed) == 0 {
                choose_aarch64_kernel();
                if AARCH64_KERNEL.load(Ordering::Relaxed) != 1 {
                    return $name::<XOROUT>(state, data).unwrap_or_default();
                }
            }

            // SAFETY: the tier's entry is the one detection selected
            let crc = unsafe { crate::arch::update(state as u64, data, &$params) as u32 };
            crc ^ XOROUT
        }
    };
}

aarch64_entry!(
    /// CRC-32/ISCSI through this CPU's fusion kernel, with `XOROUT` applied; never `None` on
    /// aarch64. Both AArch64 and x86 have native CRC-32/ISCSI instructions.
    crc32_iscsi,
    iscsi_slow,
    crc32_iscsi_with,
    crate::crc32::consts::CRC32_ISCSI
);

aarch64_entry!(
    /// CRC-32/ISO-HDLC through this CPU's fusion kernel, with `XOROUT` applied; never `None` on
    /// aarch64. Only AArch64 has native CRC-32/ISO-HDLC instructions.
    crc32_iso_hdlc,
    iso_hdlc_slow,
    crc32_iso_hdlc_with,
    crate::crc32::consts::CRC32_ISO_HDLC
);

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

/// CRC-32/ISCSI through this CPU's plan, with `XOROUT` applied to the result: the CRC32
/// instruction alone for a short buffer, the fusion kernel after that, or the folding path where
/// the plan has it take that.
///
/// It is never `None` on x86. What the caller inlines is the short arm, written so that it
/// inlines, and a jump to [`crc32_iscsi_planned`] for everything else: looking up the plan,
/// making it on the first call, the kernel and the folding path. Applying `XOROUT` on both arms,
/// rather than in the caller after them, leaves the caller nothing to do after that call, so it
/// is a tail call: the call site carries no fold of its own, keeps nothing live across a call,
/// and sets up no frame, on the short arm or the other.
#[inline(always)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
pub(crate) fn crc32_iscsi<const XOROUT: u32>(state: u32, data: &[u8]) -> Option<u32> {
    use core::sync::atomic::Ordering;

    if data.len() < ISCSI_DIRECT_END.load(Ordering::Relaxed) {
        // SAFETY: the limit is nonzero only once a kernel was selected, after detecting SSE4.2
        return Some(unsafe { x86::crc32_iscsi_direct(state, data) } ^ XOROUT);
    }

    Some(crc32_iscsi_planned::<XOROUT>(state, data))
}

/// [`crc32_iscsi`] past its short arm, with `XOROUT` applied: the plan's kernel, or the folding
/// path for the lengths the plan folds. Before the plan is made it goes to
/// [`iscsi_first_call`]; that and the folding path are jumps out of here, so the only call it
/// makes on its way to the kernel is the kernel.
#[inline(never)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
fn crc32_iscsi_planned<const XOROUT: u32>(state: u32, data: &[u8]) -> u32 {
    let Some(plan) = ISCSI_PLAN.get() else {
        return iscsi_first_call::<XOROUT>(state, data);
    };

    if let Some(plan) = plan {
        if data.len() <= plan.direct_max {
            // Only on calls that loaded `ISCSI_DIRECT_END` before the plan published it
            // SAFETY: a kernel is only selected after detecting SSE4.2
            return unsafe { x86::crc32_iscsi_small_unaligned(state, data) } ^ XOROUT;
        }

        if data.len() < plan.fold_min || data.len() >= plan.fold_end {
            // SAFETY: the kernel was selected for the tier this CPU runs, which has the
            // carry-less multiply it needs, after detecting the CRC32 instructions (and AVX2 for
            // its AVX2 build)
            return unsafe { (plan.kernel)(state, data) } ^ XOROUT;
        }
    }

    iscsi_fold::<XOROUT>(state, data)
}

/// [`crc32_iscsi_planned`] before the plan is made: makes it, once, publishes its direct limit,
/// and goes back.
#[cold]
#[inline(never)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
fn iscsi_first_call<const XOROUT: u32>(state: u32, data: &[u8]) -> u32 {
    use core::sync::atomic::Ordering;

    ISCSI_PLAN.get_or_init(|| {
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

    crc32_iscsi_planned::<XOROUT>(state, data)
}

/// [`crc32_iscsi_planned`]'s folding path, out of line: inlined, the tier's dispatch it carries
/// would have every call to the kernel save registers first.
#[inline(never)]
#[cfg(all(feature = "std", any(target_arch = "x86", target_arch = "x86_64")))]
fn iscsi_fold<const XOROUT: u32>(state: u32, data: &[u8]) -> u32 {
    // SAFETY: the tier's entry is the one detection selected
    let crc = unsafe {
        crate::arch::update(state as u64, data, &crate::crc32::consts::CRC32_ISCSI) as u32
    };
    crc ^ XOROUT
}
