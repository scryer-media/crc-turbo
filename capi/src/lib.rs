//! `libcrc_turbo`: the C library build of `crc-turbo`.
//!
//! The `crc_turbo_*` functions declared in `libcrc_turbo.h` are defined in
//! `crc-turbo`'s `ffi` module (on by default), and exported from this package's
//! shared and static libraries. Building the C library here, rather than as
//! extra crate types of `crc-turbo` itself, is what keeps a Rust dependency on
//! `crc-turbo` an ordinary rlib: Cargo builds every crate type a library
//! declares even when it is only a dependency, and a `staticlib` needs a panic
//! handler that an embedded `no_std` application provides for itself.

// Links `crc-turbo`, whose unmangled C functions are what this library exports.
pub use turbo::*;
