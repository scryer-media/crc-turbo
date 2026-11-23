// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! This module provides x86-specific implementations of the ArchOps trait.

#![cfg(any(target_arch = "x86", target_arch = "x86_64"))]

pub mod sse;

/// `keys` with their values hidden from the optimizer, for a caller whose parameters are a
/// constant. Known, each 64-bit key a fold multiplies by alone is built in a general register
/// and moved into the vector unit, on the port the multiplies use; hidden, it is a load.
#[inline(always)]
// The block neither reads nor writes through the pointer; it only passes it through.
#[allow(clippy::pointers_in_nomem_asm_block)]
pub(crate) fn opaque_keys(keys: &[u64; 23]) -> &[u64; 23] {
    #[cfg(miri)]
    {
        keys
    }
    #[cfg(not(miri))]
    {
        let mut ptr: *const [u64; 23] = keys;
        // SAFETY: the empty template leaves the pointer as it was, so it still points at
        // `keys` for their lifetime; the block only stops the optimizer seeing through it.
        unsafe {
            core::arch::asm!(
                "/* {0} */",
                inout(reg) ptr,
                options(pure, nomem, nostack, preserves_flags)
            );
            &*ptr
        }
    }
}
