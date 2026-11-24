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
    ($state:expr, $bytes:expr, $params:expr, $ops:expr, $head:literal) => {{
        let (state, bytes, params, ops) = ($state, $bytes, $params, $ops);
        macro_rules! by_width {
            () => {
                match params.width {
                    64 => algorithm::update_with::<_, Width64, $head>(state, bytes, params, &ops),
                    32 => algorithm::update_with::<_, Width32, $head>(
                        state as u32,
                        bytes,
                        params,
                        &ops,
                    ) as u64,
                    16 => algorithm::update_with::<_, Width16, $head>(
                        state as u16,
                        bytes,
                        params,
                        &ops,
                    ) as u64,
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

/// Where a tier's CRC-32/ISO-HDLC entry hands over from the fold that takes the buffer whole to
/// the general one.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const MID_END: usize = 4096;

/// The body of a tier's CRC-32/ISO-HDLC update entry, in the published form: from 128 bytes a
/// jump to the tier's folds, below that the short ladder in line. With `$short`, a ladder of
/// [`crate::arch::x86::short_asm`], from 16 bytes up to `$short_end` it goes there first.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
macro_rules! x86_iso_hdlc_body {
    ($crc:expr, $bytes:expr, $mid:ident, $fold:ident, $new_ops:expr
     $(, $short:path, $short_end:expr)?) => {{
        let (crc, bytes): (u32, &[u8]) = ($crc, $bytes);
        $(
            if bytes.len() >= 16 && bytes.len() < $short_end {
                return $short(crc, bytes) as _;
            }
        )?
        if bytes.len() >= 128 {
            if bytes.len() >= MID_END {
                return $fold(crc, bytes) as _;
            }
            return $mid(crc, bytes) as _;
        }
        !algorithm::update_short_ladder::<_, Width32>(
            !crc,
            bytes,
            &crate::crc32::consts::CRC32_ISO_HDLC,
            &$new_ops,
        )
    }};
}

/// One x86 tier, built as [`aarch64_tier!`] is: buffers shorter than 128 bytes inline in the
/// entry point, longer ones through an out-of-line fold with the tier's features, each bit
/// order compiled apart. With the folds inlined into the entry point, every call saved six
/// general registers and set up a 280-byte frame before looking at the length.
///
/// `$short_features` are the entry's, `$features` the folds'; `$head` takes long buffers to a
/// 64-byte boundary first (`algorithm::update_with`), and `$opaque_keys` has CRC-32/ISO-HDLC
/// from 128 bytes load its keys (`algorithm::update_mid`).
///
/// CRC-32/ISO-HDLC's entries take and return the CRC in its published form, the register's
/// complement, as zlib's `crc32` does: its initial value and its final XOR are both all ones,
/// so a checksum is the entry from 0 with nothing to do after it returns, and the entry and
/// each of its folds end in a tail call rather than a call and a complement.
macro_rules! x86_tier {
    ($(#[$cfg:meta])* $update:ident, $fold:ident, $iso_hdlc:ident, $iso_hdlc_checksum:ident,
     $iso_hdlc_mid:ident, $iso_hdlc_fold:ident, $short_features:literal, $features:literal,
     $head:literal, $opaque_keys:literal, $ops:ty, $new_ops:expr
     $(, short: $short:path, $short_end:expr, $iso_short:path, $iso_checksum:ident)?) => {
        $(#[$cfg])*
        #[inline]
        #[target_feature(enable = $short_features)]
        unsafe fn $update(state: u64, bytes: &[u8], params: &CrcParams, ops: $ops) -> u64 {
            $(
                if bytes.len() >= 16 && bytes.len() < $short_end {
                    return $short(state, bytes, params);
                }
            )?
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
            x86_by_order!(state, bytes, params, ops, $head)
        }

        x86_iso_hdlc_tier!(
            $(#[$cfg])* $iso_hdlc, $iso_hdlc_checksum, $iso_hdlc_mid, $iso_hdlc_fold,
            $short_features, $features, $head, $opaque_keys, $new_ops
            $(, $iso_short, $short_end, $iso_checksum)?
        );
    };
}

/// A tier's CRC-32/ISO-HDLC checksum entry: the one built with the tier's ladder in
/// [`crate::arch::x86::short_asm`] when it has one, otherwise the update entry's body from no
/// CRC, with the initial value a constant: written out again rather than called, which LLVM
/// declined to inline.
macro_rules! x86_iso_hdlc_checksum {
    ($(#[$cfg:meta])* $iso_hdlc_checksum:ident, $iso_hdlc_mid:ident, $iso_hdlc_fold:ident,
     $short_features:literal, $new_ops:expr, $checksum:ident) => {
        $(#[$cfg])*
        use crate::arch::x86::short_asm::$checksum as $iso_hdlc_checksum;
    };
    ($(#[$cfg:meta])* $iso_hdlc_checksum:ident, $iso_hdlc_mid:ident, $iso_hdlc_fold:ident,
     $short_features:literal, $new_ops:expr) => {
        $(#[$cfg])*
        #[target_feature(enable = $short_features)]
        unsafe fn $iso_hdlc_checksum(bytes: &[u8]) -> u64 {
            x86_iso_hdlc_body!(0, bytes, $iso_hdlc_mid, $iso_hdlc_fold, $new_ops) as u64
        }
    };
}

/// One x86 tier's CRC-32/ISO-HDLC entries, as [`x86_tier!`] describes them. Also built on its
/// own for the SSE tier with AVX's encoding, see [`choose_iso_hdlc`].
macro_rules! x86_iso_hdlc_tier {
    ($(#[$cfg:meta])* $iso_hdlc:ident, $iso_hdlc_checksum:ident, $iso_hdlc_mid:ident,
     $iso_hdlc_fold:ident, $short_features:literal, $features:literal, $head:literal,
     $opaque_keys:literal, $new_ops:expr
     $(, $short:path, $short_end:expr, $checksum:ident)?) => {
        /// CRC-32/ISO-HDLC with its parameters a constant, so that the width, the bit order
        /// and the keys are folded in rather than read and branched on, and each fold
        /// coefficient is one load rather than two moves into the vector unit and a pack.
        /// `crc` is the published CRC so far, 0 for none.
        $(#[$cfg])*
        #[inline]
        #[target_feature(enable = $short_features)]
        unsafe fn $iso_hdlc(crc: u32, bytes: &[u8]) -> u32 {
            x86_iso_hdlc_body!(
                crc,
                bytes,
                $iso_hdlc_mid,
                $iso_hdlc_fold,
                $new_ops
                $(, $short, $short_end)?
            )
        }

        x86_iso_hdlc_checksum!(
            $(#[$cfg])* $iso_hdlc_checksum, $iso_hdlc_mid, $iso_hdlc_fold, $short_features,
            $new_ops $(, $checksum)?
        );

        /// 128 bytes up to `MID_END`, folded from where they start and nothing else, so that
        /// they pay neither for the entry's short arms nor for the long fold's set-up. The CRC
        /// comes back widened, so that the checksum entry jumps here rather than calling.
        $(#[$cfg])*
        #[inline(never)]
        #[target_feature(enable = $features)]
        unsafe fn $iso_hdlc_mid(crc: u32, bytes: &[u8]) -> u64 {
            !algorithm::update_mid::<_, Width32, $opaque_keys>(
                !crc,
                bytes,
                &crate::crc32::consts::CRC32_ISO_HDLC,
                &$new_ops,
            ) as u64
        }

        $(#[$cfg])*
        #[inline(never)]
        #[target_feature(enable = $features)]
        unsafe fn $iso_hdlc_fold(crc: u32, bytes: &[u8]) -> u64 {
            !algorithm::update_with::<_, Width32, $head>(
                !crc,
                bytes,
                &crate::crc32::consts::CRC32_ISO_HDLC,
                &$new_ops,
            ) as u64
        }
    };
}

/// A CRC-32/ISO-HDLC checksum entry: the published CRC of the bytes, widened as
/// [`crate::checksum`] returns it, so that a caller returning it there has nothing to do after
/// the entry returns.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
type IsoHdlcChecksum = unsafe fn(&[u8]) -> u64;

/// A CRC-32/ISO-HDLC update entry: the published CRC so far and the bytes, to the published CRC.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
type IsoHdlcUpdate = unsafe fn(u32, &[u8]) -> u32;

/// The published form the entries work in is the register's complement, which holds only while
/// the initial value and the final XOR are both all ones and the bits are reflected.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const _: () = {
    let p = &crate::crc32::consts::CRC32_ISO_HDLC;
    assert!(p.init_algorithm == 0xffff_ffff && p.xorout == 0xffff_ffff && p.refin);
};

/// The tier's CRC-32/ISO-HDLC checksum entry, null until the first call chooses it. Only ever
/// set, with a relaxed store, to an entry of the tier detection selected, and every thread
/// that sets it sets the same one; the pointer is all it publishes. A checksum is then a load,
/// a test and a jump to the entry, with no frame and nothing to do after it returns.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
static ISO_HDLC_CHECKSUM: core::sync::atomic::AtomicPtr<()> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// [`ISO_HDLC_CHECKSUM`]'s update entry, chosen with it.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
static ISO_HDLC_UPDATE: core::sync::atomic::AtomicPtr<()> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// CRC-32/ISO-HDLC's entries for the tier detection selected, and records them.
#[cold]
#[inline(never)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn choose_iso_hdlc() -> (IsoHdlcChecksum, IsoHdlcUpdate) {
    use crate::feature_detection::{get_arch_ops, ArchOpsInstance};
    use core::sync::atomic::Ordering;

    let entries: (IsoHdlcChecksum, IsoHdlcUpdate) = match get_arch_ops() {
        #[cfg(all(feature = "std", target_arch = "x86_64"))]
        ArchOpsInstance::X86_64Avx512Vpclmulqdq(_)
            if crate::crc32::fusion::is_sapphire_rapids() =>
        {
            (
                iso_hdlc_checksum_x86_64_avx512_spr,
                iso_hdlc_x86_64_avx512_spr,
            )
        }
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx512Vpclmulqdq(_) => (
            iso_hdlc_checksum_x86_64_avx512_vpclmulqdq,
            iso_hdlc_x86_64_avx512_vpclmulqdq,
        ),
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx512Pclmulqdq(_) => (
            iso_hdlc_checksum_x86_64_avx512_pclmulqdq,
            iso_hdlc_x86_64_avx512_pclmulqdq,
        ),
        #[cfg(all(feature = "std", target_arch = "x86_64"))]
        ArchOpsInstance::X86_64Avx2Vpclmulqdq(_) if crate::crc32::fusion::is_intel_ecore() => (
            iso_hdlc_checksum_x86_64_avx2_ecore,
            iso_hdlc_x86_64_avx2_ecore,
        ),
        #[cfg(target_arch = "x86_64")]
        ArchOpsInstance::X86_64Avx2Vpclmulqdq(_) => (
            iso_hdlc_checksum_x86_64_avx2_vpclmulqdq,
            iso_hdlc_x86_64_avx2_vpclmulqdq,
        ),
        ArchOpsInstance::X86SsePclmulqdq(_) if sse_tier_has_avx() => {
            (iso_hdlc_checksum_x86_sse_avx, iso_hdlc_x86_sse_avx)
        }
        ArchOpsInstance::X86SsePclmulqdq(_) => (
            iso_hdlc_checksum_x86_sse_pclmulqdq,
            iso_hdlc_x86_sse_pclmulqdq,
        ),
        ArchOpsInstance::SoftwareFallback => (iso_hdlc_checksum_software, iso_hdlc_software),
    };
    ISO_HDLC_UPDATE.store(entries.1 as *mut (), Ordering::Relaxed);
    ISO_HDLC_CHECKSUM.store(entries.0 as *mut (), Ordering::Relaxed);

    entries
}

/// The `x86_64-avx2-vpclmulqdq` tier's CRC-32/ISO-HDLC from 256 bytes up to [`MID_END`] on
/// Intel's E-core parts: libdeflate's cascade, [`x86_64::iso_hdlc_cascade`], in place of the
/// tier's `iso_hdlc_mid`. Below 256 bytes the tier's ladder stays ahead of it there.
#[cfg(all(feature = "std", target_arch = "x86_64"))]
use x86_64::iso_hdlc_cascade::iso_hdlc_cascade as iso_hdlc_mid_x86_64_avx2_ecore;

/// The E-core parts' checksum entry: the tier's ladder in line, then the cascade for 256 bytes
/// to [`MID_END`] in place of the tier's mid fold, and the tier's long fold after that.
#[cfg(all(feature = "std", target_arch = "x86_64"))]
use crate::arch::x86::short_asm::iso_hdlc_checksum_avx2_ecore as iso_hdlc_checksum_x86_64_avx2_ecore;

/// [`iso_hdlc_x86_64_avx2_vpclmulqdq`] on Intel's E-core parts: the same entry, with the
/// cascade for 256 bytes to [`MID_END`].
#[cfg(all(feature = "std", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "avx2,vpclmulqdq,sse4.1,pclmulqdq")]
unsafe fn iso_hdlc_x86_64_avx2_ecore(crc: u32, bytes: &[u8]) -> u32 {
    if bytes.len() >= 16 && bytes.len() < 128 {
        return crate::arch::x86::short_asm::iso_hdlc_short128_avx(crc, bytes) as u32;
    }
    if bytes.len() >= 256 && bytes.len() < MID_END {
        return iso_hdlc_mid_x86_64_avx2_ecore(crc, bytes) as u32;
    }
    iso_hdlc_x86_64_avx2_vpclmulqdq(crc, bytes)
}

/// Sapphire Rapids' CRC-32/ISO-HDLC on the `x86_64-avx512-vpclmulqdq` tier from 256 bytes up
/// to [`MID_END`]: the 512-bit cascade with four accumulators, fast-crc32's
/// `avx512_vpclmulqdq_crc32_v4` loop, [`x86_64::iso_hdlc_fold512`]. Below 256 bytes the tier's
/// own mid fold. On a Xeon Platinum 8488C, against libdeflate in the same run, 2 KiB went from
/// 0.95x to 0.99x and 512 bytes from 1.06x to 1.12x; on a Xeon Platinum 8375C (Ice Lake) and an
/// EPYC 9R14 (Zen 4) it was no faster than the tier's fold, and up to 3% slower, so they keep it.
#[cfg(all(feature = "std", target_arch = "x86_64"))]
#[inline(never)]
#[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq")]
unsafe fn iso_hdlc_mid_x86_64_avx512_spr(crc: u32, bytes: &[u8]) -> u64 {
    if bytes.len() >= 256 {
        return x86_64::iso_hdlc_fold512::iso_hdlc_fold512::<false>(crc, bytes);
    }
    iso_hdlc_mid_x86_64_avx512_vpclmulqdq(crc, bytes)
}

/// Sapphire Rapids' CRC-32/ISO-HDLC from [`MID_END`]: below [`SPR_EIGHT_END`] the 512-bit
/// cascade with eight accumulators over 512-byte steps, libdeflate's and fast-crc32's
/// `avx512_vpclmulqdq_crc32_v8` loop, and the tier's long fold after that.
#[cfg(all(feature = "std", target_arch = "x86_64"))]
#[inline(never)]
#[target_feature(enable = "avx512vl,avx512bw,vpclmulqdq")]
unsafe fn iso_hdlc_fold_x86_64_avx512_spr(crc: u32, bytes: &[u8]) -> u64 {
    if bytes.len() < SPR_EIGHT_END {
        return x86_64::iso_hdlc_fold512::iso_hdlc_fold512::<true>(crc, bytes);
    }
    iso_hdlc_fold_x86_64_avx512_vpclmulqdq(crc, bytes)
}

/// Where Sapphire Rapids' eight-accumulator cascade hands over to the tier's long fold. On the
/// Xeon Platinum 8488C, against libdeflate in the same run, it took 4 KiB from 0.97x to 0.98x
/// and 8 KiB from 0.97x to 1.02x, and was behind the tier's fold at 16 KiB, 0.97x to 0.99x.
#[cfg(all(feature = "std", target_arch = "x86_64"))]
const SPR_EIGHT_END: usize = 16 << 10;

/// Sapphire Rapids' checksum entry: the tier's ladder in line, then
/// [`iso_hdlc_mid_x86_64_avx512_spr`] and [`iso_hdlc_fold_x86_64_avx512_spr`].
#[cfg(all(feature = "std", target_arch = "x86_64"))]
use crate::arch::x86::short_asm::iso_hdlc_checksum_avx512_spr as iso_hdlc_checksum_x86_64_avx512_spr;

/// [`iso_hdlc_x86_64_avx512_vpclmulqdq`] on Sapphire Rapids: the same entry, with the 512-bit
/// cascade from 256 bytes.
#[cfg(all(feature = "std", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "avx512vl,vpclmulqdq")]
unsafe fn iso_hdlc_x86_64_avx512_spr(crc: u32, bytes: &[u8]) -> u32 {
    x86_iso_hdlc_body!(
        crc,
        bytes,
        iso_hdlc_mid_x86_64_avx512_spr,
        iso_hdlc_fold_x86_64_avx512_spr,
        crate::arch::x86_64::avx512_vpclmulqdq::X86_64Avx512VpclmulqdqOps::new(),
        crate::arch::x86::short_asm::iso_hdlc_short128_avx,
        128
    )
}

/// Whether a CPU on the SSE tier has AVX, so that CRC-32/ISO-HDLC takes the SSE tier's entries
/// built with AVX's three-operand encoding: the same 128-bit operations, without the register
/// copies that the two-operand forms need before each multiply. The CPUs that land on the SSE
/// tier with AVX are those with AVX2 or AVX but no VPCLMULQDQ, such as a Xeon E5-2666 v3 or an
/// EPYC 7R32; libdeflate takes its AVX build of the same fold on them.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn sse_tier_has_avx() -> bool {
    #[cfg(feature = "std")]
    {
        std::arch::is_x86_feature_detected!("avx")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(target_feature = "avx")
    }
}

/// CRC-32/ISO-HDLC through the table-driven software path, in the entries' published form.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
unsafe fn iso_hdlc_software(crc: u32, bytes: &[u8]) -> u32 {
    !(crate::arch::software::update((!crc) as u64, bytes, &crate::crc32::consts::CRC32_ISO_HDLC)
        as u32)
}

/// [`iso_hdlc_software`] from no CRC.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
unsafe fn iso_hdlc_checksum_software(bytes: &[u8]) -> u64 {
    iso_hdlc_software(0, bytes) as u64
}

/// The CRC-32/ISO-HDLC checksum of `bytes`, through the entry of the tier detection selected.
///
/// x86 has no CRC32 instruction for this polynomial, so it folds like any other algorithm;
/// through [`update`] each call reads the width, bit order and keys from the parameters and
/// branches on them, which a short buffer pays for in full. What a caller inlines is a load of
/// the entry and a jump to it; the first call chooses it out of line.
#[inline(always)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) fn crc32_iso_hdlc(bytes: &[u8]) -> u64 {
    let entry = ISO_HDLC_CHECKSUM.load(core::sync::atomic::Ordering::Relaxed);
    if entry.is_null() {
        return crc32_iso_hdlc_first(bytes);
    }
    // SAFETY: a non-null value was stored by `choose_iso_hdlc` from an `IsoHdlcChecksum` of
    // the tier detection selected, whose features this CPU has
    unsafe { core::mem::transmute::<*mut (), IsoHdlcChecksum>(entry)(bytes) }
}

/// [`crc32_iso_hdlc`] for the first call.
#[cold]
#[inline(never)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn crc32_iso_hdlc_first(bytes: &[u8]) -> u64 {
    // SAFETY: the entry is the one for the tier detection selected
    unsafe { choose_iso_hdlc().0(bytes) }
}

/// [`update`] for CRC-32/ISO-HDLC: `state` is the register, as [`update`] takes it.
#[inline(always)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) fn update_iso_hdlc(state: u32, bytes: &[u8]) -> u32 {
    let entry = ISO_HDLC_UPDATE.load(core::sync::atomic::Ordering::Relaxed);
    let entry = if entry.is_null() {
        choose_iso_hdlc().1
    } else {
        // SAFETY: as in `crc32_iso_hdlc`, from an `IsoHdlcUpdate`
        unsafe { core::mem::transmute::<*mut (), IsoHdlcUpdate>(entry) }
    };
    // SAFETY: the entry is the one for the tier detection selected
    !unsafe { entry(!state, bytes) }
}

x86_tier!(
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    update_x86_sse_pclmulqdq,
    fold_x86_sse_pclmulqdq,
    iso_hdlc_x86_sse_pclmulqdq,
    iso_hdlc_checksum_x86_sse_pclmulqdq,
    iso_hdlc_mid_x86_sse_pclmulqdq,
    iso_hdlc_fold_x86_sse_pclmulqdq,
    "sse4.1,pclmulqdq",
    "sse4.1,pclmulqdq",
    false,
    true,
    crate::arch::x86::sse::X86SsePclmulqdqOps,
    crate::arch::x86::sse::X86SsePclmulqdqOps,
    short: crate::arch::x86::short_asm::short_sse,
    256,
    crate::arch::x86::short_asm::iso_hdlc_short_sse,
    iso_hdlc_checksum_sse
);
x86_iso_hdlc_tier!(
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    iso_hdlc_x86_sse_avx,
    iso_hdlc_checksum_x86_sse_avx,
    iso_hdlc_mid_x86_sse_avx,
    iso_hdlc_fold_x86_sse_avx,
    "sse4.1,pclmulqdq,avx",
    "sse4.1,pclmulqdq,avx",
    false,
    true,
    crate::arch::x86::sse::X86SsePclmulqdqOps,
    crate::arch::x86::short_asm::iso_hdlc_short_avx,
    256,
    iso_hdlc_checksum_avx
);
x86_tier!(
    #[cfg(target_arch = "x86_64")]
    update_x86_64_avx512_pclmulqdq,
    fold_x86_64_avx512_pclmulqdq,
    iso_hdlc_x86_64_avx512_pclmulqdq,
    iso_hdlc_checksum_x86_64_avx512_pclmulqdq,
    iso_hdlc_mid_x86_64_avx512_pclmulqdq,
    iso_hdlc_fold_x86_64_avx512_pclmulqdq,
    "avx512vl,pclmulqdq",
    "avx512vl,pclmulqdq",
    false,
    true,
    crate::arch::x86_64::avx512::X86_64Avx512PclmulqdqOps,
    crate::arch::x86_64::avx512::X86_64Avx512PclmulqdqOps::new(),
    short: crate::arch::x86::short_asm::short128_avx,
    128,
    crate::arch::x86::short_asm::iso_hdlc_short128_avx,
    iso_hdlc_checksum_avx512_pclmulqdq
);
x86_tier!(
    #[cfg(target_arch = "x86_64")]
    update_x86_64_avx512_vpclmulqdq,
    fold_x86_64_avx512_vpclmulqdq,
    iso_hdlc_x86_64_avx512_vpclmulqdq,
    iso_hdlc_checksum_x86_64_avx512_vpclmulqdq,
    iso_hdlc_mid_x86_64_avx512_vpclmulqdq,
    iso_hdlc_fold_x86_64_avx512_vpclmulqdq,
    "avx512vl,vpclmulqdq",
    "avx512vl,avx512bw,vpclmulqdq",
    true,
    false,
    crate::arch::x86_64::avx512_vpclmulqdq::X86_64Avx512VpclmulqdqOps,
    crate::arch::x86_64::avx512_vpclmulqdq::X86_64Avx512VpclmulqdqOps::new(),
    short: crate::arch::x86::short_asm::short128_avx,
    128,
    crate::arch::x86::short_asm::iso_hdlc_short128_avx,
    iso_hdlc_checksum_avx512_vpclmulqdq
);
x86_tier!(
    #[cfg(target_arch = "x86_64")]
    update_x86_64_avx2_vpclmulqdq,
    fold_x86_64_avx2_vpclmulqdq,
    iso_hdlc_x86_64_avx2_vpclmulqdq,
    iso_hdlc_checksum_x86_64_avx2_vpclmulqdq,
    iso_hdlc_mid_x86_64_avx2_vpclmulqdq,
    iso_hdlc_fold_x86_64_avx2_vpclmulqdq,
    "avx2,vpclmulqdq,sse4.1,pclmulqdq",
    "avx2,vpclmulqdq,sse4.1,pclmulqdq",
    false,
    false,
    crate::arch::x86_64::avx2_vpclmulqdq::X86_64Avx2VpclmulqdqOps,
    crate::arch::x86_64::avx2_vpclmulqdq::X86_64Avx2VpclmulqdqOps::new(),
    short: crate::arch::x86::short_asm::short128_avx,
    128,
    crate::arch::x86::short_asm::iso_hdlc_short128_avx,
    iso_hdlc_checksum_avx2_vpclmulqdq
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
        // Long enough for the buffers that are split at a 64-byte boundary first
        const LONG: usize = 64 << 10;
        let mut data = vec![0u8; LONG + 512];
        rng().fill(&mut data[..]);

        let mut lengths: Vec<usize> = (0..=64).collect();
        lengths.extend(120..=400);
        lengths.extend([511, 512, 513, 1023, 1024, 1025, 4095, 4096, 4097, 8192]);
        lengths.extend([LONG - 1, LONG, LONG + 1, LONG + 63, LONG + 129, LONG + 300]);

        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let init = config.get_init_algorithm();
            let carried = crate::arch::software::update(init, &data[8192..8192 + 7], params);

            for offset in [0usize, 1, 15, 33, 63, 64] {
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
        ($(#[$cfg:meta])* $test:ident, $update:ident, $iso_hdlc:ident, $iso_hdlc_checksum:ident,
         $ops:expr, [$($feature:tt),+]) => {
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
                check_iso_hdlc_matches_software(
                    |state, bytes| unsafe { !$iso_hdlc(!state, bytes) },
                    |bytes| unsafe { $iso_hdlc_checksum(bytes) },
                );
            }
        };
    }

    /// A tier's CRC-32/ISO-HDLC entries against the software path, at every length below the
    /// fold and the fold's boundaries above it, from four starts: the update entry, taking and
    /// returning the register, from the init state and a carried state, and the checksum entry.
    #[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "std"))]
    fn check_iso_hdlc_matches_software(
        tier: impl Fn(u32, &[u8]) -> u32,
        checksum: impl Fn(&[u8]) -> u64,
    ) {
        let params = &crate::crc32::consts::CRC32_ISO_HDLC;
        let mut data = vec![0u8; 8192 + 64];
        rng().fill(&mut data[..]);
        let init = params.init_algorithm;
        let carried = crate::arch::software::update(init, &data[8192..8192 + 7], params);

        let mut lengths: Vec<usize> = (0..=600).collect();
        lengths.extend([1023, 1024, 1025, 4095, 4096, 4097, 8192]);
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
                let expected = crate::arch::software::update(init, input, params) ^ params.xorout;
                assert_eq!(
                    checksum(input),
                    expected,
                    "checksum offset {offset} len {len}"
                );
            }
        }
    }

    x86_tier_test!(
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        test_sse_pclmulqdq_tier_matches_software,
        update_x86_sse_pclmulqdq,
        iso_hdlc_x86_sse_pclmulqdq,
        iso_hdlc_checksum_x86_sse_pclmulqdq,
        crate::arch::x86::sse::X86SsePclmulqdqOps,
        ["sse4.1", "pclmulqdq"]
    );
    /// The SSE tier's CRC-32/ISO-HDLC entries built with AVX's encoding, forced on any CPU with
    /// AVX.
    #[test]
    #[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), feature = "std"))]
    #[cfg_attr(miri, ignore)]
    fn test_sse_avx_iso_hdlc_matches_software() {
        if !(is_x86_feature_detected!("sse4.1")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("avx"))
        {
            eprintln!("skipping test_sse_avx_iso_hdlc_matches_software: the CPU lacks AVX");
            return;
        }
        check_iso_hdlc_matches_software(
            |state, bytes| unsafe { !iso_hdlc_x86_sse_avx(!state, bytes) },
            |bytes| unsafe { iso_hdlc_checksum_x86_sse_avx(bytes) },
        );
    }

    /// The E-core parts' CRC-32/ISO-HDLC entries on the `x86_64-avx2-vpclmulqdq` tier, forced on
    /// any CPU with its features, against the table-driven software path.
    #[test]
    #[cfg(all(target_arch = "x86_64", feature = "std"))]
    #[cfg_attr(miri, ignore)]
    fn test_avx2_ecore_iso_hdlc_matches_software() {
        if !(is_x86_feature_detected!("avx2")
            && is_x86_feature_detected!("vpclmulqdq")
            && is_x86_feature_detected!("sse4.1")
            && is_x86_feature_detected!("pclmulqdq"))
        {
            eprintln!(
                "skipping test_avx2_ecore_iso_hdlc_matches_software: the CPU lacks VPCLMULQDQ"
            );
            return;
        }
        check_iso_hdlc_matches_software(
            |state, bytes| unsafe { !iso_hdlc_x86_64_avx2_ecore(!state, bytes) },
            |bytes| unsafe { iso_hdlc_checksum_x86_64_avx2_ecore(bytes) },
        );
    }

    /// Sapphire Rapids' CRC-32/ISO-HDLC entries on the `x86_64-avx512-vpclmulqdq` tier, forced
    /// on any CPU with its features, against the table-driven software path.
    #[test]
    #[cfg(all(target_arch = "x86_64", feature = "std"))]
    #[cfg_attr(miri, ignore)]
    fn test_avx512_spr_iso_hdlc_tier_matches_software() {
        if !(is_x86_feature_detected!("avx512vl")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("vpclmulqdq"))
        {
            eprintln!(
                "skipping test_avx512_spr_iso_hdlc_tier_matches_software: the CPU lacks VPCLMULQDQ"
            );
            return;
        }
        check_iso_hdlc_matches_software(
            |state, bytes| unsafe { !iso_hdlc_x86_64_avx512_spr(!state, bytes) },
            |bytes| unsafe { iso_hdlc_checksum_x86_64_avx512_spr(bytes) },
        );
    }

    x86_tier_test!(
        #[cfg(target_arch = "x86_64")]
        test_avx2_vpclmulqdq_tier_matches_software,
        update_x86_64_avx2_vpclmulqdq,
        iso_hdlc_x86_64_avx2_vpclmulqdq,
        iso_hdlc_checksum_x86_64_avx2_vpclmulqdq,
        crate::arch::x86_64::avx2_vpclmulqdq::X86_64Avx2VpclmulqdqOps::default(),
        ["avx2", "vpclmulqdq", "sse4.1", "pclmulqdq"]
    );
    x86_tier_test!(
        #[cfg(target_arch = "x86_64")]
        test_avx512_pclmulqdq_tier_matches_software,
        update_x86_64_avx512_pclmulqdq,
        iso_hdlc_x86_64_avx512_pclmulqdq,
        iso_hdlc_checksum_x86_64_avx512_pclmulqdq,
        crate::arch::x86_64::avx512::X86_64Avx512PclmulqdqOps::default(),
        ["avx512vl", "pclmulqdq"]
    );
    x86_tier_test!(
        #[cfg(target_arch = "x86_64")]
        test_avx512_vpclmulqdq_tier_matches_software,
        update_x86_64_avx512_vpclmulqdq,
        iso_hdlc_x86_64_avx512_vpclmulqdq,
        iso_hdlc_checksum_x86_64_avx512_vpclmulqdq,
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

    /// The split-buffer cutoff is chosen per target: Apple's cores keep the unaligned fold to
    /// 256 KiB, other aarch64 parts split a buffer from 64 KiB.
    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_aarch64_unaligned_fold_end_per_vendor() {
        let want = if cfg!(target_vendor = "apple") {
            256 << 10
        } else {
            64 << 10
        };
        assert_eq!(crate::algorithm::UNALIGNED_FOLD_END, want);
    }

    /// Forces each aarch64 tier, the SHA3 one on any CPU that has it whatever dispatch picks, and
    /// checks every algorithm, both bit orders, against the table-driven software implementation
    /// across the 128-byte fold at every chunk count from two to nine and the unaligned head and
    /// tail, from the init state and a carried state. Lengths either side of
    /// `UNALIGNED_FOLD_END`, from an aligned and an unaligned start, take the unaligned fold and
    /// the split buffer.
    #[test]
    #[cfg(all(target_arch = "aarch64", feature = "std"))]
    #[cfg_attr(miri, ignore)]
    fn test_aarch64_tiers_match_software() {
        if !std::arch::is_aarch64_feature_detected!("aes") {
            eprintln!("skipping test_aarch64_tiers_match_software: no AES");
            return;
        }
        let sha3 = std::arch::is_aarch64_feature_detected!("sha3");

        let end = crate::algorithm::UNALIGNED_FOLD_END;
        let long = [end - 1, end, end + 1, end + 77];
        let mut data = vec![0u8; (end + 77).max(8192) + 64];
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

            for offset in [0usize, 1, 15, 33, 63, 64] {
                // The long lengths from an aligned and an unaligned start only, for time.
                let long: &[usize] = if offset <= 1 { &long } else { &[] };
                for &len in lengths.iter().chain(long) {
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
