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
  `clippy -D warnings` across feature sets, docs, MSRV and packaging, and a
  `wasm-test` lane that runs the test suite on `wasm32-wasip1` and
  `wasm32-wasip2` under wasmtime and on `wasm32-unknown-emscripten` under
  Node, where upstream only compiled for WASM; `security`
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

- CRC-32/ISCSI on the tiers below AVX-512 takes its shift multipliers from
  tables built at compile time. crc-fast's fusion kernels work them out for
  every buffer by repeated squaring (`xnmodp`), which on a Xeon 6731E
  (Sierra Forest) cost about 90 cycles of a 264-byte checksum's 152 and
  about 270 of a 4 KiB one's 602. The SSE kernel is also a different shape,
  `v4x2s4x4` in fast-crc32's naming where crc-fast has `v4s3x3`, with its
  two halves sized in 32-byte units so that no more than 31 bytes of a
  pass are left to the CRC32 instruction alone, and it takes over from the
  plain CRC32 instruction loop at 200 bytes rather than 257; `v4s3x3`
  is kept for buffers over 3 MiB, where reading one place fewer at a time
  is worth more. The AVX-512 and aarch64 fusion kernels still compute
  their shifts per buffer.
- CRC-32/ISCSI buffers under 200 bytes are checksummed with the CRC32
  instruction over unaligned words, by a direct call ahead of the fusion
  kernel; crc-fast aligns first, a byte at a time, and so does this crate
  through 1.11.1.
- The fusion kernel for CRC-32/ISCSI, and on aarch64 for CRC-32/ISO-HDLC,
  is chosen once per process and kept as a function pointer. crc-fast, and
  this crate through 1.11.1, ran feature detection on every checksum call:
  up to four atomic loads before the first byte.

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
  From 1.11.1 the `no_std` build's compile-time detection requires it too;
  in 1.11.0 only the runtime detection of `std` builds did.
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

## 1.11.2 - 2026-11-21

- CRC-32/ISCSI is faster on x86 CPUs without AVX-512: the
  `x86_64-avx2-vpclmulqdq`, `x86_64-sse-pclmulqdq` and `x86-sse-pclmulqdq`
  tiers. Its fusion kernel shifts five partial CRCs back together at the end
  of a buffer, and worked out the multipliers for that by repeated squaring
  on every call. They now come from tables built at compile time (8 KiB for
  passes up to 32 KiB, and four entries for each doubling beyond that; a
  longer buffer is taken in a few passes, largest first). Measured on a
  Xeon 6731E (Sierra Forest, which selects `x86_64-avx2-vpclmulqdq`)
  against 1.11.1, user-mode cycles per call, median of six interleaved
  rounds, checksums compared first, the build timed being this release's
  source:

  | bytes | 1.11.1 | 1.11.2 | |
  |---:|---:|---:|---:|
  | 8 | 24.5 | 18.0 | 1.36x |
  | 16 | 29.0 | 18.4 | 1.58x |
  | 64 | 24.9 | 18.5 | 1.34x |
  | 128 | 38.0 | 34.0 | 1.12x |
  | 191 | 73.4 | 59.0 | 1.24x |
  | 264 | 151.2 | 58.2 | 2.60x |
  | 1024 | 278.8 | 125.0 | 2.23x |
  | 4096 | 602.4 | 342.3 | 1.76x |
  | 16384 | 1629 | 1209 | 1.35x |
  | 65536 | 5386 | 4958 | 1.09x |
  | 1048576 | 77878 | 76400 | 1.02x |
  | 4194304 | 358125 | 347063 | 1.03x |

  That is 6.8 to 12.0 bytes per cycle at 4 KiB and 13.5 to 13.7 at 1 MiB.
  Of the 34 lengths from 1 to 512 bytes timed separately, from an aligned
  start and from an unaligned one, and the 35 from 8 bytes to 4 MiB above,
  none was slower; the closest was 65 bytes from an aligned start, 23.8 to
  23.0. From 4 MiB to 128 MiB the two releases run the same kernel and
  measured within 1%. CRC-32/ISO-HDLC and CRC-64/NVME, which do not use this
  kernel, measured the same in both releases, and the same held with the
  tier pinned to `x86_64-sse-pclmulqdq`. Those figures are from a build in
  which the kernel took over at 200 bytes; it now takes over at 257 (below),
  and 200 to 256 bytes run the CRC32 instruction loop, as in 1.11.1, which
  was not timed again on this CPU.

  On an EPYC 7R13 (Zen 3, a c6a.xlarge on EC2, which also selects
  `x86_64-avx2-vpclmulqdq`), nanoseconds per call, median of nine
  interleaved rounds, checksums compared first and the test suite run on
  it: 64 bytes 7.50 to 6.11 (11.96 to 6.11 unaligned), 256 bytes 18.33 to
  17.50, 257 bytes 37.59 to 24.75, 4 KiB 253 to 196, 1 MiB 1.05x, and the
  same from 4 MiB up. None of 22 lengths from 1 to 512 bytes, aligned or
  unaligned, was slower. Alder Lake through Arrow Lake and the other x86
  CPUs with SSE4.2 and PCLMULQDQ but no AVX-512 were not measured. The
  AVX-512 tiers and aarch64 keep their kernels and still compute their
  shifts per buffer.
- The SSE kernel itself is now the `v4x2s4x4` shape of
  [fast-crc32](https://github.com/corsix/fast-crc32/): two 64-byte vector
  steps beside four CRC32 instruction streams, in place of `v4s3x3`. On the
  Xeon 6731E the shape alone was worth 3% at 1 MiB and nothing at 4 KiB;
  the tables above are where the time was. Its fold keys are the generated
  ones (keys 16 and 15, 20 and 19, 2 and 1), its halves are sized in
  32-byte units so no more than 31 bytes of a pass fall to the CRC32
  instruction alone, and it takes over from the plain CRC32 instruction
  loop at 257 bytes, as `v4s3x3` did. On the Xeon 6731E it overtook the
  loop at 200 bytes, but on the EPYC 7R13 a build that took over there was
  up to 1.5x slower than 1.11.1 from 200 to 256 bytes (200 bytes 14.72 to
  22.56 ns).
- `v4s3x3` stays for buffers over 3 MiB. `v4x2s4x4` reads five places in a
  buffer at once, and on the Xeon 6731E, whose cores share 4 MiB of L2
  cache, it was 2% behind `v4s3x3` at 4 MiB and 12% to 15% behind from
  8 MiB to 128 MiB, whatever the pass size; software prefetch recovered
  part of that and cost the lengths that fit in the cache.
- CRC-32/ISCSI buffers up to 256 bytes no longer align before the CRC32
  instruction loop. The loop takes unaligned 8-byte words from the start
  of the buffer and finishes with at most one each of four, two and one
  bytes, where it took up to seven single bytes at each end. On the Xeon
  6731E 64 bytes takes 18.5 cycles from any start; 1.11.1 took 22.4 to
  25.3 from an aligned start, run to run, and 44.0 from an unaligned one.
  These buffers are a direct call ahead of the kernel's function pointer,
  on every x86 tier with the CRC32 instructions, and the kernels are kept
  out of line so that a short buffer does not pay for the registers a
  kernel saves. That changes the AVX-512 tiers too, for buffers up to 256
  bytes, so they were timed on EC2 against 1.11.1: a c6i.xlarge (Xeon
  Platinum 8375C, Ice Lake, `x86_64-avx512-vpclmulqdq`) and a c5.xlarge
  (Xeon Platinum 8124M, Skylake-SP, `x86_64-avx512-pclmulqdq`), nanoseconds
  per call, median of nine interleaved rounds, checksums compared first
  (lengths 0 to 300 and larger, aligned and unaligned, on each tier pinned
  down to software), and the full test suite run on both:

  | bytes | 8375C 1.11.1 | 8375C 1.11.2 | 8124M 1.11.1 | 8124M 1.11.2 |
  |---:|---:|---:|---:|---:|
  | 8 | 7.45 | 5.44 | 9.43 | 8.14 |
  | 64 | 6.86 | 5.43 | 8.97 | 8.09 |
  | 64, unaligned | 14.33 | 5.43 | 15.14 | 8.09 |
  | 199 | 12.58 | 8.85 | 18.44 | 12.31 |
  | 256 | 12.76 | 11.20 | 19.27 | 17.84 |
  | 257 | 13.41 | 12.43 | 44.94 | 41.30 |
  | 4096 | 95.19 | 94.67 | 160.54 | 155.82 |

  Of the 36 lengths from 1 to 512 bytes, aligned and unaligned, none was
  slower on either CPU; 512 bytes on the 8375C read 0.97x once and 1.00x
  in a rerun of 15 rounds. From 1 KiB to 1 MiB 1.11.2 was 1.00x to 1.04x.
  From 2 MiB to 64 MiB both CPUs moved by up to 5% either way between runs,
  following which release ran first, and CRC-32/ISO-HDLC and CRC-64/NVME,
  whose code did not change, moved by as much. Those runs had the direct
  call stop at 199 bytes. With it at 256 the 8375C was timed again, and
  200 to 256 bytes ran 1.16x to 1.27x faster than 1.11.1 (256 bytes 12.79
  to 10.09 ns, 19.00 to 10.11 unaligned), with no length from 1 to 512
  bytes slower; the 8124M was not timed again.
- The fusion kernel is chosen once per process, from the tier and the CRC32
  instructions, and kept as a function pointer; a checksum call loads the
  pointer and calls it. Before, every CRC-32/ISCSI call on x86 and every
  CRC-32/ISCSI and CRC-32/ISO-HDLC call on aarch64 ran feature detection
  first, up to four atomic loads. On a Graviton2 (Neoverse N1, a
  c6g.xlarge on EC2, `aarch64-neon-pmull`), checksums matched 1.11.1 and the
  test suite passed; CRC-32/ISCSI and CRC-32/ISO-HDLC were 1.03x to 1.35x
  faster from 1 to 512 bytes, aligned and unaligned (the closest, 200
  bytes, 1.00x), and the same from 4 KiB up. Apple CPUs were not timed.
- These changes come from reports by cemeyer, crc-turbo#3 (dispatch) and
  crc-turbo#4 (the Sierra Forest gap, with the measurements against
  fast-crc32's kernels that showed it).

## 1.11.1 - 2026-11-08

- A `no_std` build for x86_64 chose the AVX-512 tiers from its compile-time
  target features on AVX512VL alone. Those kernels also use AVX512BW, so a
  target that enables AVX512VL without AVX512BW, as for a guest whose
  hypervisor masks it, reached instructions the CPU may not have. It now
  takes the tiers below, as `std` builds already did; both detection paths
  decide through one rule. `std` builds are unchanged. The gap is item 3 of
  crc-fast#64 by GiorgioRegni, which 1.11.0 closed for `std` builds only.

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