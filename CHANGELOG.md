# Changelog

All notable changes to this project are documented in this file.

## Fork

`crc-turbo` began as a fork of
[crc-fast](https://github.com/awesomized/crc-fast-rust) by Don MacAskill,
taken at its 1.10.0 release (`3a853cc`). crc-fast is a good library whose
maintainer has been inactive since that release: no merges or comments since
the start of 2026, and a backlog of outside pull requests - dependency
security bumps, `no_std` fixes, `digest` 0.11, a 256-bit x86 tier - waiting
without an answer. This crate exists so that work can ship. It is a hard
fork: it is not rebased onto crc-fast and is not kept in step with it, though
a fix that lands there is still welcome here. Every change taken from one of
crc-fast's open pull requests or issues credits its author below and in the
commit that carries it. This section is the record of how the crate differs from
the release it was taken at, kept for readers who know crc-fast. crc-fast's
own changelog up to the fork point continues below, unchanged.

### Packaging

- Crate renamed to `crc-turbo`, library `crc_turbo`. The Rust module paths
  and the public API stay crc-fast 1.10.0's, so a consumer's migration is the
  dependency line plus `crc_fast::` → `crc_turbo::`.
- Version starts at `1.11.0`, one minor above the upstream base, to make the
  lineage obvious. The MSRV stays crc-fast's 1.89; development and CI use the
  toolchain pinned in `rust-toolchain.toml`.
- The C library's header is `libcrc_turbo.h`, its symbols are `crc_turbo_*`
  and its types `CrcTurbo*`. With crc-fast's names, a binary linking both
  crates under the default `ffi` feature would fail on duplicate symbols.
- Released by `.github/workflows/release.yml`, dispatched from the Actions tab
  on `main` (it releases the version `Cargo.toml` names and creates the tag)
  or started by the signed `v<version>` tag
  `cargo xtask release` pushes; crates.io trusted publishing either way. See
  `docs/publishing.md`. Upstream's release workflow, which also built and
  attested binary packages of the C library and the CLI tools, is not
  carried; the `Makefile` builds and installs both from a checkout.
- Repository hygiene adopted from the scryer-media house style: SHA-pinned CI
  ending in a required `ci-complete` job that keeps all of upstream's lanes
  (aarch64, x86_64, 32-bit x86, the software fallback on big-endian PowerPC,
  Miri, `no_std`, WASM and a fuzz smoke of every target) and adds a `hygiene`
  lane (no binaries, home paths or credential-shaped strings in the tree),
  `clippy -D warnings` across feature sets, docs, MSRV and packaging; `security`
  (cargo-audit over both lockfiles, zizmor); `codeql`; gitleaks pre-commit
  hooks; renovate; issue templates; `AGENTS.md`, `SECURITY.md`,
  `CONTRIBUTORS.md`. Upstream's `tests.yml` and `release.yml` were replaced
  as duplicates of these. The Linux lanes run on `ubuntu-26.04` and
  `ubuntu-26.04-arm`, the current hosted images.
- The crate archive no longer carries the repository's CI, hook, fuzz and
  agent files, nor upstream's `.kiro/` design notes, which stay in the
  repository.

### Fixes taken from crc-fast's open pull requests

- `no_std` builds for x86_64 compile again: the AVX-512 tier imported
  `std::arch` rather than `core::arch` (crc-fast#57, reported by
  MuntasirSZN). From crc-fast#42 by netsbot; crc-fast#49 by onur-ozkan
  carries the same fix.
- A `crc-turbo` dependency is an ordinary Rust library again. The crate
  declared `cdylib` and `staticlib` crate types as well, which Cargo builds
  even for a dependency, and a `no_std` static library needs a panic handler
  the embedded application provides for itself. The C library, renamed
  `libcrc_turbo`, is now built by the unpublished `capi/` package; the
  Makefile builds and installs from there, and `make capi-test` runs a C smoke
  test against the static and the shared library. From crc-fast#49 by
  onur-ozkan, which dropped the extra crate types; the `capi/` package keeps
  the C library. `wasm-pack`, which packages only a `cdylib`, no longer
  builds the crate itself, so upstream's browser-package CI step is gone; a
  browser package wraps the crate in a `cdylib` crate of its own.
- The `ffi` feature enables `std`. The C functions use `std` throughout,
  so `--no-default-features --features ffi` did not compile. From
  crc-fast#48 by MggMuggins.
- The licence field is `(MIT OR Apache-2.0) AND Zlib`: `src/combine.rs`
  carries Mark Adler's Zlib-licensed `crccomb.c`, now as the reference its
  tests check against. The `capi/` package carries none of it and stays
  `MIT OR Apache-2.0`. From crc-fast#41 by musicinmybrain.

### Binary size

- New `generated-tables` feature: the fifty built-in lookup tables
  (664 KiB) are generated on first use instead of stored, so a binary that
  picks its algorithm at run time no longer carries every table. On
  aarch64 a release build choosing among three algorithms shrinks from
  1,062,560 to 385,584 bytes. The tables are those the stored ones were
  generated from, and a test checks every built-in algorithm against the
  generator either way. Requested in crc-fast#61 by jacobloveless, whose
  per-width table features this replaces with one feature that covers
  every algorithm a binary does not use, without fifty feature flags.

### digest 0.11

- New `digest-0-11` feature: `Digest` implements digest 0.11's `DynDigest`
  as well as digest 0.10's, from one shared body. digest 0.10 stays the
  default because its `DynDigest` impl is reached under the default
  features (`std` enables `alloc`), so moving the default to 0.11 changes a
  trait a consumer already uses without opting in, which is a breaking
  change for 2.0. The update itself is crc-fast#46 by xtqqczze, for
  crc-fast#44 by Nukesor; taking it as a second, opt-in implementation lets
  it ship in 1.11.

### Dependencies

- `spin` 0.10.1; 0.10.0 is yanked (crc-fast#52). From crc-fast#50 by
  xtqqczze.
- `crossbeam-epoch` 0.9.20 in the development lockfile, for RUSTSEC-2026-0204
  (crc-fast#53). From crc-fast#55 by xtqqczze.
- `rand` 0.10 for the tests and benches, and `rand` 0.9.5 under `proptest`,
  for RUSTSEC-2026-0097 (crc-fast#54). From crc-fast#48 by MggMuggins, which
  supersedes crc-fast#51 by xtqqczze. `rand` was never a dependency of the
  library itself.
- `cargo audit` is clean on the crate, `fuzz/` and `capi/` lockfiles.

### x86_64 performance

- A new `x86_64-avx2-vpclmulqdq` tier for CPUs that have 256-bit VPCLMULQDQ
  but no AVX-512 (AMD Zen 3; Intel Alder Lake through Arrow Lake, where
  AVX-512 is fused off), which crc-fast runs on its 128-bit SSE tier. It
  holds the SSE tier's eight 128-bit fold streams in four YMM registers, so
  each carry-less multiply folds two streams, and byte-reverses forward
  (non-reflected) CRCs within each 128-bit lane, so no load needs a
  lane-crossing permute. It reuses the existing fold keys and reduction, so
  it covers every CRC-16, CRC-32 and CRC-64 algorithm in both bit orders;
  CRC-32/ISCSI keeps its SSE4.2 fusion path. AVX-512 CPUs keep their tiers.
  A test forces the tier on any CPU that has it and checks every algorithm
  against the table-driven implementation from unaligned starts and a
  carried state. VPCLMULQDQ is now detected on its own rather than only
  under AVX-512VL. dcbtsla proposed the same tier to crc-fast in
  crc-fast#56, independently and earlier; the feature-detection model here
  (VPCLMULQDQ and AVX2 as separate flags) is taken from it.

### Tier selection

- The `CRC_TURBO_TIER` environment variable (exported as `TIER_ENV`, `std`
  only) pins the tier to a target string such as `x86_64-sse-pclmulqdq`, to
  compare tiers on one binary. It never selects a tier the CPU cannot run.
  An integration test runs the CLI tools under every tier string and checks
  the check values and the selection rule.
- Forcing the software tier on an aarch64 CPU with PMULL no longer panics.
- The pin covers the CRC-32/ISCSI and CRC-32/ISO-HDLC fusion kernels, which
  chose their form from the CPU alone: `x86_64-sse-pclmulqdq` or
  `x86_64-avx2-vpclmulqdq` on an AVX-512 part now runs the SSE kernel, and
  `aarch64-neon-pmull` on a CPU with SHA3 the PMULL kernel. A test hashes a
  64 KiB buffer under every tier against the software tier, so every fold
  and fusion kernel a tier can reach is compared, not only the 9-byte check
  string.

### Fixes found in the pre-release review

- The C library no longer aborts the process on a caller-supplied value.
  A custom algorithm variant passed to a function that takes an algorithm
  alone (`crc_turbo_checksum`, `crc_turbo_digest_new`, …) sets the new
  `UnsupportedAlgorithm` error and returns 0 or NULL instead of panicking
  across the FFI boundary; a `CrcTurboParams` with a width other than 16,
  32 or 64 is rejected with `UnsupportedWidth` before it reaches the
  library, and `crc_turbo_get_custom_params` returns keyless parameters for
  such a width instead of panicking. Width 16, which the library supports,
  is no longer reported as unsupported.
- Two undefined-behaviour paths in the C library are closed: the key array
  of a `CrcTurboParams` was turned into a slice of `key_count` elements
  before the count was checked, and `crc_turbo_digest_combine` with the
  same handle twice held a mutable and a shared reference to one digest.
  The key pointer is also checked for alignment.
- `crc_turbo_error_message` returns the static strings it documents rather
  than leaking a copy per call, and the new `crc_turbo_free_string` releases
  the string `crc_turbo_get_calculator_target` allocates.
- The aarch64 CRC-32 EOR3 kernels computed their loop limit as an address
  before the buffer when the input was under 384 bytes, which Miri reports
  as undefined behaviour (the address was compared, never read). It is
  wrapping arithmetic now. Upstream's Miri lane runs on x86_64 only, so it
  never saw this.
- The combine field asserts a width of exactly 16, 32 or 64, as its
  documentation says, instead of accepting any width from 16 to 64.
- `checksum_combine` masks both checksums to the CRC width; crc-fast 1.10
  passed stray high bits of a malformed `checksum2` through. Catalogued
  inputs are unaffected.
- The carry-less multiply behind the combine operator lives in
  `src/arch/clmul.rs`, reached through a token that only detection builds,
  so `src/combine.rs` holds no `unsafe`.
- `make capi-test` and the other non-install targets run on macOS without
  `DESTDIR`; only `make install` requires it.
- The crate archive no longer carries the `Makefile` and `docs/`, which
  refer to the repository's `capi/` package.
- `cargo xtask release --publish` refuses `--skip-tests` and a HEAD that is
  not on `origin/main`.
- The CLI test that asserted a measured duration of one second asserts only
  that the duration is printed. The CLI tests run the built `checksum`
  binary rather than a nested `cargo run`, which re-linked it under
  concurrent tests and killed the siblings executing it. Where the target has
  a runner (`CARGO_TARGET_<TRIPLE>_RUNNER`, as under `cross test` on the
  PowerPC lanes, where the binary is of a foreign architecture), the tests
  run the binary through it. The tier pin test checks that every tier below
  the detected one is selected when pinned, not merely accepted.
- A `CrcTurboParams` with `refin` differing from `refout` is rejected with
  the new `MismatchedReflection` error. It reached an assertion in the
  combine field, which aborted the process, and the plain checksum ignored
  `refout`.
- Custom reflected CRC-32 and CRC-64 parameters start from the reflected
  `init`, as the Rocksoft model says and as the CRC-16 path already did.
  crc-fast reversed the init for width 16 only, so a reflected 32- or
  64-bit custom CRC whose init is not a bit palindrome checksummed wrongly;
  no catalogued CRC of those widths is affected, as they all start from 0
  or all ones.
- The aarch64 CRC-32/ISCSI and CRC-32/ISO-HDLC fusion kernels, which use
  the CRC32 instructions, run only on a CPU that reports the `crc`
  extension; it is optional on ARMv8.0 and crc-fast never checked it.
- The AVX-512 tiers also require AVX512BW, which their byte shuffles use.
- `CRC_TURBO_TIER=x86_64-avx512-pclmulqdq` on a part with VPCLMULQDQ holds
  CRC-32/ISCSI to the PCLMULQDQ kernel; the kernel re-detected VPCLMULQDQ
  and ran the wider one.
- `.gitleaks.toml` extends the default rule set; a config without
  `[extend]` has no rules at all, so the pre-commit scan could never find
  a secret. The hook also fails closed when `git diff --cached` fails.
- crc-fast#64 by GiorgioRegni, filed the day before that review, reports
  the aarch64 `crc` and AVX512BW gaps above and two more, also fixed: the
  AVX-512 CRC-32/ISCSI kernel folded the initial CRC in through
  `_mm512_castsi128_si512`, whose upper 384 bits the intrinsic leaves
  undefined (the compiler zeroed them, so no result was wrong); it is now
  `_mm512_zextsi128_si512`, which defines them as zero. And the tests it
  asked for exist: every built-in algorithm against the `crc` crate from
  every start offset within a cache line, inputs that end exactly at an
  unmapped page and inputs that start exactly after one (a read outside
  the input faults the test), and a digest split at every point of 3,000
  bytes.

### Errors

- Misuse no longer has to panic. New `CrcError` (`UnsupportedWidth`,
  `MismatchedReflection`, `MissingParams`), with fallible forms of every
  entry point that could panic on its arguments: `CrcParams::try_new`,
  `try_checksum`, `Digest::try_new`, `Digest::try_new_with_init_state`,
  `CrcCombine::try_new` and `CrcCombine::try_new_with_params`. The
  existing functions keep their signatures and now document their panics,
  which carry the same messages. `CrcAlgorithm` derives `Eq` and `Hash`.
  Requested in crc-fast#37 by dengyt2018 for `no_std` callers; the error
  type is hand-written rather than `thiserror`'s, so the library gains no
  dependency.

### Combine

- `checksum_combine` and `checksum_combine_with_params` compute the shift
  as a power of x modulo the polynomial with a carry-less multiply
  (PCLMULQDQ on x86_64, PMULL on aarch64, a portable shift-and-add
  elsewhere), in place of crc-fast's GF(2) matrix squaring. Each built-in
  algorithm keeps a table of x^(2^k), built on its first combine, so a
  combine costs one multiply per set bit of the length, as zlib's
  `crc32_combine` does; custom parameters use square-and-multiply. The
  table needs `std`, or `alloc` on aarch64 and x86. The results are
  bit-identical; the old matrix code is kept as the test oracle. On an
  Apple M-series core, one combine of a CRC-32/ISO-HDLC over a 1 MiB second
  part drops from 12.5 µs to 12 ns, and CRC-64/NVME from 81.7 µs to 13 ns.
  The gain ranges from 18x for a CRC-16 over 1 byte to 6,300x for a CRC-64
  over 1 MiB. The field arithmetic is from crc-fast#60 by dcbtsla. That
  pull request's per-polynomial key tables and its new `CrcParams` field
  are not taken: the field would be a breaking change to a struct
  consumers build by hand, and the tables add several kilobytes that
  crc-fast#61 asks to shrink.
- New `CrcCombine`, a combine operator for one algorithm and one length,
  built once with `CrcCombine::new` or `CrcCombine::new_with_params`. Each
  `combine` is then one field multiply, about 2 ns. `uncombine` runs it
  backwards: from the CRC of `A ‖ B` and the CRC of `B`, it returns the CRC
  of `A`. `zeros` returns the CRC of that many zero bytes.
- New `checksum_zeros` and `checksum_zeros_with_params`: the CRC of `len`
  zero bytes in O(log len), without touching memory.

## 1.11.0 - 2026-10-20

The first release of the fork. It is everything in the
[Fork](https://github.com/scryer-media/crc-turbo/blob/main/CHANGELOG.md#fork)
section of `CHANGELOG.md`.

## [1.10.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.10.0) - 2025-12-26
* [Rust 1.89 baseline](https://github.com/awesomized/crc-fast-rust/pull/27)
* [Remove 'crc' dependency](https://github.com/awesomized/crc-fast-rust/pull/40)

## [1.9.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.9.0) - 2025-12-22
* [Optimize performance for smaller sizes](https://github.com/awesomized/crc-fast-rust/pull/38)
* [Add CRC-16 support](https://github.com/awesomized/crc-fast-rust/pull/39)

## [1.8.2](https://github.com/awesomized/crc-fast-rust/releases/tag/1.8.2) - 2025-12-14
* [Fix the crc dependency minimum](https://github.com/awesomized/crc-fast-rust/pull/33)
* [Fix no_std build for embedded targets](https://github.com/awesomized/crc-fast-rust/pull/35)

## [1.8.1](https://github.com/awesomized/crc-fast-rust/releases/tag/1.8.1) - 2025-11-16
* [Add support for fuzz testing using libFuzzer](https://github.com/awesomized/crc-fast-rust/pull/30)

## [1.8.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.8.0) - 2025-11-14
* [Add no_std and WASM support with full backward compatibility](https://github.com/awesomized/crc-fast-rust/pull/28)
* [Use feature_detection instead of func calls for fusion](https://github.com/awesomized/crc-fast-rust/commit/3af5cdbec9cf85ed5ce325c131845514873e2e3e)
* [Improve test and release coverage for x86](https://github.com/awesomized/crc-fast-rust/commit/bba891b97cc5cb32b3320e717749f76e2f46c053)

## [1.7.1](https://github.com/awesomized/crc-fast-rust/releases/tag/1.7.1) - 2025-11-10
* [Ensure Miri passes on x86_64 and x86](https://github.com/awesomized/crc-fast-rust/pull/26)

## [1.7.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.7.0) - 2025-11-07
* [Fix/no std feature (currently wasm compatible; groundwork for no_std)](https://github.com/awesomized/crc-fast-rust/pull/25)
* Support and publish [immutable releases](https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/) on GitHub

## [1.6.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.6.0) - 2025-10-30
* [Improve runtime feature detection (and performance)](https://github.com/awesomized/crc-fast-rust/pull/21)
* [remove libc](https://github.com/awesomized/crc-fast-rust/pull/20)
* [Enable generating and publishing binary packages](https://github.com/awesomized/crc-fast-rust/pull/22)

## [1.5.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.5.0) - 2025-09-01
* [Improve state handling](https://github.com/awesomized/crc-fast-rust/pull/16)
* [Add support for building a static library](https://github.com/awesomized/crc-fast-rust/pull/17)

## [1.4.1](https://github.com/awesomized/crc-fast-rust/releases/tag/1.4.1) - 2025-09-01
* [change unconditional x86-64-v4 reliance to the former x86-64-v2 reliance](https://github.com/awesomized/crc-fast-rust/pull/15)

## [1.4.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.4.0) - 2025-08-08
* [Enable VPCLMULQDQ support on Rust 1.89+](https://github.com/awesomized/crc-fast-rust/pull/10)
* [Support custom CRC parameters](https://github.com/awesomized/crc-fast-rust/pull/11)
* [Add checksum command-line utility](https://github.com/awesomized/crc-fast-rust/pull/12)
* [Remove bindgen](https://github.com/awesomized/crc-fast-rust/pull/13)

## [1.3.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.3.0) - 2025-06-10
* [Replace C bindings for CRC32 fusion calculation](https://github.com/awesomized/crc-fast-rust/pull/9)
* [Improve VPCLMULQDQ to use 512-bit wide registers](https://github.com/awesomized/crc-fast-rust/pull/8)
* [Implement hardware accelerated XOR3 support](https://github.com/awesomized/crc-fast-rust/pull/9)

## [1.2.2](https://github.com/awesomized/crc-fast-rust/releases/tag/1.2.2) - 2025-06-02
* [Remove println! from software fallback](https://github.com/awesomized/crc-fast-rust/pull/4)

## [1.2.1](https://github.com/awesomized/crc-fast-rust/releases/tag/1.2.1) - 2025-05-10
* [Limit FFI to supported architectures](https://github.com/awesomized/crc-fast-rust/commit/55b967bf623953879fdce74447a9b84f820ac879)

## [1.2.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.2.0) - 2025-05-08
* [Add table-based software fallback](https://github.com/awesomized/crc-fast-rust/commit/9432876eb47e322a35046485b498e18053f889f9)

## [1.1.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.1.0) - 2025-05-02
* [Add digest::DynDigest::box_clone() and Debug support](https://github.com/awesomized/crc-fast-rust/commit/8a494c30ef8ff640ddb113d9fe171611dfb211e5)

## [1.0.1](https://github.com/awesomized/crc-fast-rust/releases/tag/1.0.1) - 2025-04-30
* [Use Rust 1.81+](https://github.com/awesomized/crc-fast-rust/pull/1)

## [1.0.0](https://github.com/awesomized/crc-fast-rust/releases/tag/1.0.0) - 2025-04-10
- First release for crates.io