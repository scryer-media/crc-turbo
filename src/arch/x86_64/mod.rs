// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! This module provides x86_64-specific implementations of the ArchOps trait.

#![cfg(target_arch = "x86_64")]

pub mod avx2_vpclmulqdq;
pub mod avx512;
pub mod avx512_vpclmulqdq;
#[cfg(feature = "std")]
pub(crate) mod iso_hdlc_cascade;
#[cfg(feature = "std")]
pub(crate) mod iso_hdlc_fold512;
