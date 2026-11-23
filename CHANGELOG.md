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
- `cargo xtask codegen check` holds the hot paths' compiled shape to
  `codegen/<target>.snap`: the inline assembly of each listed function, by
  mnemonic, and the count of instructions and calls the compiler generated
  around it. CI runs it on the pinned toolchain, on the MSRV and as a
  dependent crate builds the library (no LTO, default codegen units), on
  x86_64 and aarch64; `codegen-watch.yml` runs it weekly on beta and nightly.

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
- On targets without an accelerated path (WebAssembly, and every
  architecture but x86, x86_64 and aarch64) each built-in algorithm has a
  calculator of its own that names its table, so a module using one
  algorithm links one table even without LTO: a `no_std` WebAssembly module
  computing CRC-64/NVME is 38 KiB, from 718 KiB (767 KiB to 56 KiB with
  `std`). Custom parameters without the `cache` feature no longer leak a
  table on every call, and their table and the `generated-tables` ones are
  built in place on the heap rather than on the stack. CI builds that module
  without LTO and fails above 64 KiB. From crc-fast#67 by Andres J Ruiz
  Torres, without its slice-by-16 loop.

### WebAssembly

- On WebAssembly, every generator with a sparse multiple of the right shape
  (all but CRC-32/XFER and six of the seven CRC-64 generators) skips the
  tables for most of a long input, so 43 of the 50 built-in algorithms are
  faster there, up to 4.3x for CRC-16/DNP at 64 KiB. See 1.11.1.

### x86 and aarch64

- CRC-32/ISCSI follows a per-CPU plan below AVX-512, its fusion kernels
  take their shifts from compile-time tables on x86 and on aarch64's SHA3
  tier, the kernels below AVX-512 have an AVX2 build, and x86 buffers under
  4 KiB take a 256-bit short fold, one final reduction and independent tail
  folds, after the Linux kernel's and libdeflate's methods. The kernel is
  chosen once per process. See 1.11.1.

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
  pass are left to the CRC32 instruction alone; `v4s3x3` is kept for
  buffers over 3 MiB (128 KiB on Alder Lake's AVX2 build), where reading
  one place fewer at a time is worth more. Both are built twice, and on x86_64 CPUs with AVX2 the build with
  AVX2 enabled runs, whose VEX encoding was measured faster on every CPU
  timed. The aarch64 SHA3 tier's CRC-32/ISCSI and CRC-32/ISO-HDLC
  kernels for buffers from 2 KiB take theirs from tables too; the AVX-512
  fusion kernel still computes its shifts per buffer.
- Below AVX-512, CRC-32/ISCSI follows a plan chosen per CPU model: the
  plain CRC32 instruction loop runs up to 384 bytes (320 on AMD family
  0x19, 288 on family 0x17, 240 on Haswell, 199 on Intel's E-core parts),
  where crc-fast hands over to its kernel at 257 on every CPU, and on AMD
  family 0x19 the `x86_64-avx2-vpclmulqdq` tier's 256-bit fold takes
  buffers from 4 KiB instead of the fusion kernel.
- On Haswell, Zen 2, Zen 3, the AVX-512 PCLMULQDQ tier, Sapphire Rapids,
  Ice Lake and Zen 4, a window of lengths past the CRC32 instruction's
  limit takes a kernel of carry-less folds alone (fast-crc32's `v4`); on
  Zen 2 buffers from 32 KiB take three CRC32 streams alone (fast-crc32's
  `s3`). crc-fast has neither.
- CRC-32/ISCSI buffers up to the plan's limit (256 bytes on the AVX-512
  tiers) are checksummed with the CRC32 instruction over unaligned words,
  written as inline assembly inside the caller on x86_64, ahead of the
  fusion kernel; crc-fast aligns first, a byte at a time, and so does this
  crate through 1.11.0. The entry is frame-free and tail-jumps to the
  plan, and the `x86_64-avx512-vpclmulqdq` `v3x2` kernel aligns and
  finishes in inline assembly, so their shape does not depend on the
  compiler version; crc-fast's are compiler-chosen.
- The fusion kernel for CRC-32/ISCSI, and on aarch64 for CRC-32/ISO-HDLC,
  is chosen once per process: on x86 kept as a function pointer, on aarch64
  as one byte ahead of direct calls. crc-fast, and this crate through
  1.11.0, ran feature detection on every checksum call: up to four atomic
  loads before the first byte.

- AVX-512 tiers: the two 512-bit registers a fold ends with are reduced to
  128 bits in one step with per-lane generated keys; on the
  `x86_64-avx512-vpclmulqdq` tier 192 to 255 bytes fold their first 64
  bytes 128 bytes on before that step, and folds from 64 KiB start at a
  64-byte boundary; and CRC-32/ISCSI has a kernel of four 512-bit folding
  streams beside five CRC32-instruction streams, selected by CPU model and
  length.
- x86: CRC-32/ISO-HDLC from 128 bytes to 4 KiB has an entry of its own on
  every tier; on the `x86-sse-pclmulqdq`, `x86_64-sse-pclmulqdq` and
  `x86_64-avx512-pclmulqdq` tiers it loads its fold keys rather than
  building them in general registers.
- x86, the 256- and 512-bit VPCLMULQDQ tiers: a buffer of whole 128-byte
  blocks goes from the fold straight to the final reduction, as fast-crc32's
  generated `v4` kernel does, without a jump on the count of blocks left.
- x86: buffers of 16 to 255 bytes (16 to 127 on the 256- and 512-bit
  tiers) are folded by inline assembly, one straight-line arm per number of
  16-byte blocks with each block folded onto the last across its own
  distance, so that their shape does not depend on the compiler; crc-fast
  folds them in a chain of compiled intrinsics. CRC-32/ISO-HDLC has a
  checksum entry per tier with that assembly in line, and on the
  `x86_64-avx2-vpclmulqdq` and `x86_64-avx512-vpclmulqdq` tiers its
  checksum folds 64 to 255 and 64 to 127 bytes at 256 and 512 bits, the
  shape of libdeflate's.

- Every architecture: 32 to 255 bytes, the generic fold no longer runs its
  16-byte blocks as one chain of dependent multiplies. Each of the last
  seven blocks is folded straight onto the last with the generated key for
  its distance, so those multiplies run side by side, the method of the
  Linux kernel's x86 CRC template (Eric Biggers); no kernel code is taken.

### aarch64 performance

- CRC-32/ISCSI and CRC-32/ISO-HDLC run fast-crc32's `v4` fold from 64
  bytes and `v8` from 256, without first aligning the buffer, where crc-fast
  runs `v12e_v1` from 128 bytes, aligning first; from 2 KiB the PMULL tier
  runs `v3s4x2e_v2` in place of `v12e_v1`, and the SHA3 tier crc-fast's
  `v9s3x2e_s3` (from 1 KiB there) with its shift multipliers from
  compile-time tables. On
  Apple's cores each multiply and the XOR that takes its result are written
  as a pair the core fuses, the SHA3 kernel's main loop included, where the
  three-way XOR crc-fast uses there keeps the core from fusing either.
- Off Apple, CRC-32/ISCSI and CRC-32/ISO-HDLC run the CRC32 instruction
  loop below 80 bytes on the SHA3 tier, and below 1 KiB on the PMULL tier,
  which then runs `v3s4x2e_v2` from 1 KiB rather than 2 KiB; the stream
  kernels finish their remainder with the same loop, and `v9s3x2e_s3`
  issues every multiply before its three-way XORs, as crc-fast does.
  Below 64 bytes every tier calls a copy of the loop without its 64-byte
  block step, so that the compiler fits each copy to its length range.
- On aarch64 Linux and Android those loops (below 64 bytes; on the SHA3
  tier below 96 bytes, on the PMULL tier below 1 KiB) and the SHA3 tier's
  four-lane fold (96 to 255 bytes) are assembly shipped in the crate, so
  every consumer's compiler builds the same instructions at the same
  64-byte alignment. The compiled loops and fold stay as the other targets'
  path and as the tests' reference.
- `checksum` naming CRC-32/ISCSI or CRC-32/ISO-HDLC is that algorithm's
  entry point inlined, not a match over every algorithm.
- On aarch64 every CRC-32/ISCSI and CRC-32/ISO-HDLC kernel applies the
  final XOR itself, as fast-crc32's kernels apply their complement, so the
  inlined entry point ends in a jump to the kernel with no frame around it.
- The other algorithms fold a buffer from 128 bytes to 64 KiB (256 KiB on
  Apple) from wherever it starts and reduce it once, where crc-fast folds
  the bytes before and after an aligned middle separately. That fold is
  kept out of line, compiled once per bit order; buffers under 128 bytes
  run crc-fast's code inlined in the tier's entry point. The 128-byte fold
  loops, this one and the aligned one, step a pointer to the end with one
  add and one compare, and a forward (not reflected) algorithm reverses
  each block's bytes with one `TBL`, where crc-fast's loop compiles to a
  `REV64` and an `EXT`.

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

## 1.11.1 - 2026-11-23

- CRC-32/ISCSI is faster on x86 CPUs without AVX-512: the
  `x86_64-avx2-vpclmulqdq`, `x86_64-sse-pclmulqdq` and `x86-sse-pclmulqdq`
  tiers. Its fusion kernel shifts five partial CRCs back together at the end
  of a buffer, and worked out the multipliers for that by repeated squaring
  on every call. They now come from tables built at compile time (8 KiB for
  passes up to 32 KiB, and four entries for each doubling beyond that; a
  longer buffer is taken in a few passes, largest first). Measured on a
  Xeon 6731E (Sierra Forest, which selects `x86_64-avx2-vpclmulqdq`)
  against 1.11.0, user-mode cycles per call, median of six interleaved
  rounds, checksums compared first, the build timed being this release's
  source:

  | bytes | 1.11.0 | 1.11.1 | |
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
  tier pinned to `x86_64-sse-pclmulqdq`. Those figures are from the SSE
  build of the kernel taking over at 200 bytes. This release runs the
  kernel's AVX2 build and the per-CPU plan below, neither of which was
  timed on this CPU; the plan keeps the 200-byte handover for it.
- Below AVX-512 the kernels are built twice, once for SSE4.2 and once with
  AVX2 enabled as well, and an x86_64 CPU with AVX2 runs the second. The
  instructions are the same 128-bit ones; with AVX2 enabled they take the
  VEX encoding, three operands and no register copies. Against the SSE
  build, median of interleaved rounds on EC2: an EPYC 7R13 (Zen 3) 4% to
  27% faster, an EPYC 7R32 (Zen 2) 2% to 14% from 512 bytes and 27% to 36%
  from 4 MiB, a Xeon E5-2666 v3 (Haswell) 12% to 27% from 257 bytes to
  1 KiB, and a Xeon Platinum 8375C pinned to `x86_64-avx2-vpclmulqdq` 0% to
  5%. Built from fast-crc32's own generated C (commit `13f5289`), the same
  shapes give the same checksums; with AVX2 flags clang's build of them
  ran as fast as this crate's, and gcc's up to 1.40x faster on the EPYC
  7R13 from 1 MiB.
- Below AVX-512, CRC-32/ISCSI follows a plan chosen once per process from
  the CPU's vendor, family and model, measured on each CPU it names:
  - the CRC32 instruction loop runs up to 384 bytes: against the kernel's
    AVX2 build it was faster through 320 bytes on the EPYC 7R13 and the
    8375C and level at 384, and on the Haswell part faster through 320
    bytes from an aligned start and 384 from an unaligned one;
  - up to 288 bytes on AMD family 0x17 (Zen to Zen 2), through which the
    loop was faster on the 7R32;
  - up to 199 bytes on Intel's E-core parts (family 6, models 0xaf Sierra
    Forest, 0xb6 Grand Ridge, 0xdd Clearwater Forest and 0xbe Alder
    Lake-N), where the Xeon 6731E's kernel won from 200;
  - on AMD family 0x19 (Zen 3, and Zen 4 pinned below AVX-512) on the
    `x86_64-avx2-vpclmulqdq` tier, buffers from 4 KiB go to that tier's
    256-bit fold instead of the fusion kernel: on the 7R13 it was 1.05x
    at 4 KiB to 1.14x from 1 MiB faster than the fusion kernel's AVX2
    build. On the 8375C, the 7R32 and the Haswell part the fold ran at
    about 0.6x the fusion kernel's speed, so they keep the kernel;
  - on Alder Lake's hybrid parts (Intel family 6, models 0x97 and 0x9a)
    the AVX2 build hands over to `v4s3x3` from 128 KiB instead of 3 MiB,
    and the loop keeps 384 bytes. Measured on a Core i5-1240P, P-core and
    E-core apart, in nanoseconds: on the P-cores the four-stream kernel
    was 1% to 2% behind 1.11.0's `v4s3x3` from 256 KiB to 2 MiB, and a
    200-byte handover up to 12% behind the loop from 200 to 256 bytes; on
    the E-cores the kernel overtook the loop at 200 bytes and the
    four-stream shape was 4% to 10% ahead. The fold was 1.02x to 1.07x
    on the P-cores from 64 KiB to 4 MiB but about 0.75x from 8 MiB and
    about 0.6x on the E-cores, so Alder Lake keeps the kernel. Against
    1.11.0, median of nine or fifteen interleaved rounds: P-cores 1.38x at
    64 bytes (3.15x unaligned), 2.0x to 2.6x from 257 bytes to 1 KiB,
    1.40x at 4 KiB, 1.04x at 64 KiB and 1.00x to 1.02x from 128 KiB to
    32 MiB; E-cores 1.26x to 3.0x up to 4 KiB, 1.18x to 1.30x from 64 KiB
    to 2 MiB and 1.03x to 1.16x from 3 MiB.

  Measured on EC2 spot instances against 1.11.0, nanoseconds per call,
  median of nine interleaved rounds pinned to one core, from an aligned
  start unless marked. Checksums of CRC-32/ISCSI, the fold with the same
  parameters, CRC-32/ISO-HDLC and CRC-64/NVME matched 1.11.0 first, over
  every length from 0 to 1,100 bytes and larger ones around 4 KiB and
  3 MiB, aligned and unaligned, with the tier pinned to each target string
  in turn; the full test suite passed on each CPU, natively and pinned to
  `x86-sse-pclmulqdq`:

  | bytes | 7R13 1.11.0 | 7R13 1.11.1 | 7R32 1.11.0 | 7R32 1.11.1 | E5-2666 v3 1.11.0 | E5-2666 v3 1.11.1 |
  |---:|---:|---:|---:|---:|---:|---:|
  | 64 | 6.4 | 4.7 | 7.9 | 5.2 | 8.7 | 5.9 |
  | 64, unaligned | 10.6 | 4.7 | 12.8 | 5.2 | 14.5 | 5.8 |
  | 256 | 18.6 | 16.2 | 25.1 | 22.1 | 17.4 | 13.6 |
  | 257 | 37.3 | 16.4 | 43.4 | 22.1 | 42.0 | 13.7 |
  | 384 | 47.4 | 28.9 | 61.1 | 32.7 | 51.8 | 22.8 |
  | 1024 | 87.3 | 53.4 | 105.2 | 58.5 | 77.6 | 36.2 |
  | 4096 | 252.9 | 159.9 | 276.8 | 182.1 | 167.1 | 101.6 |
  | 1048576 | 51664 | 38495 | 51892 | 43091 | 24946 | 24620 |
  | 4194304 | 206486 | 153751 | 208078 | 152591 | 157265 | 157143 |

  The 7R13 selects `x86_64-avx2-vpclmulqdq`, the other two
  `x86-sse-pclmulqdq`. Of the 21 lengths from 16 bytes to 4 MiB, aligned
  and from a start 3 bytes in, none was slower on any of the three; the
  closest were 1 MiB and 4 MiB on the Haswell part, 1.00x to 1.01x, where
  the same kernel runs, and 200 bytes on the 7R32, 1.10x. The 8375C pinned to `x86_64-avx2-vpclmulqdq` was 1.15x to 3.37x
  faster up to 1 KiB and 1.00x to 1.03x from 1 MiB. CRC-32/ISO-HDLC and
  CRC-64/NVME, which share none of this code, measured 0.95x to 1.08x
  across the same runs. Sierra Forest and the other E-core parts, Zen 4
  and Zen 5 without AVX-512, Alder Lake through Arrow Lake and Hygon were
  not measured with this build. The AVX-512 tiers keep their kernels and
  still compute their shifts per buffer; aarch64 is below.
- The SSE kernel itself is now the `v4x2s4x4` shape of
  [fast-crc32](https://github.com/corsix/fast-crc32/): two 64-byte vector
  steps beside four CRC32 instruction streams, in place of `v4s3x3`. On the
  Xeon 6731E the shape alone was worth 3% at 1 MiB and nothing at 4 KiB;
  the tables above are where the time was. Its fold keys are the generated
  ones (keys 16 and 15, 20 and 19, 2 and 1), its halves are sized in
  32-byte units so no more than 31 bytes of a pass fall to the CRC32
  instruction alone, and it takes over from the plain CRC32 instruction
  loop where the plan above says. On the Xeon 6731E it overtook the loop
  at 200 bytes, but on the EPYC 7R13 a build that took over there was up
  to 1.5x slower than 1.11.0 from 200 to 256 bytes (200 bytes 14.72 to
  22.56 ns), which is why the handover is per CPU.
- `v4s3x3` stays for buffers over 3 MiB. `v4x2s4x4` reads five places in a
  buffer at once, and on the Xeon 6731E, whose cores share 4 MiB of L2
  cache, it was 2% behind `v4s3x3` at 4 MiB and 12% to 15% behind from
  8 MiB to 128 MiB, whatever the pass size; software prefetch recovered
  part of that and cost the lengths that fit in the cache.
- CRC-32/ISCSI buffers up to the plan's limit, and up to 256 bytes on the
  AVX-512 tiers, no longer align before the CRC32 instruction loop. The loop takes unaligned 8-byte words from the start
  of the buffer and finishes with at most one each of four, two and one
  bytes, where it took up to seven single bytes at each end. On the Xeon
  6731E 64 bytes takes 18.5 cycles from any start; 1.11.0 took 22.4 to
  25.3 from an aligned start, run to run, and 44.0 from an unaligned one.
  These buffers are a direct call ahead of the kernel's function pointer,
  on every x86 tier with the CRC32 instructions, and the kernels are kept
  out of line so that a short buffer does not pay for the registers a
  kernel saves. That changes the AVX-512 tiers too, for buffers up to 256
  bytes, so they were timed on EC2 against 1.11.0: a c6i.xlarge (Xeon
  Platinum 8375C, Ice Lake, `x86_64-avx512-vpclmulqdq`) and a c5.xlarge
  (Xeon Platinum 8124M, Skylake-SP, `x86_64-avx512-pclmulqdq`), nanoseconds
  per call, median of nine interleaved rounds, checksums compared first
  (lengths 0 to 300 and larger, aligned and unaligned, on each tier pinned
  down to software), and the full test suite run on both:

  | bytes | 8375C 1.11.0 | 8375C 1.11.1 | 8124M 1.11.0 | 8124M 1.11.1 |
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
  in a rerun of 15 rounds. From 1 KiB to 1 MiB 1.11.1 was 1.00x to 1.04x.
  From 2 MiB to 64 MiB both CPUs moved by up to 5% either way between runs,
  following which release ran first, and CRC-32/ISO-HDLC and CRC-64/NVME,
  whose code did not change, moved by as much. Those runs had the direct
  call stop at 199 bytes. With it at 256 the 8375C was timed again, and
  200 to 256 bytes ran 1.16x to 1.27x faster than 1.11.0 (256 bytes 12.79
  to 10.09 ns, 19.00 to 10.11 unaligned), with no length from 1 to 512
  bytes slower; the 8124M was not timed again. With the final source the
  8375C and a c5.large (Xeon Platinum 8275CL, Cascade Lake,
  `x86_64-avx512-pclmulqdq`) were timed once more over 21 lengths from
  16 bytes to 4 MiB, aligned and unaligned, with the checks and suite
  above: 64 bytes 5.8 to 4.0 and 7.2 to 5.8 ns (12.9 to 4.0 and 13.7 to
  5.8 unaligned), 256 bytes 12.6 to 9.5 and 17.3 to 13.6, and 1.00x to
  1.21x from 257 bytes up; the closest, 512 bytes unaligned on the 8375C,
  was 0.99x.
- The fusion kernel is chosen once per process, from the tier and the CRC32
  instructions. On x86 it is kept as a function pointer, which a checksum
  call loads and calls. On aarch64 the choice is one byte, read with a
  relaxed load, and the call to the CRC32 loop or the fold is direct, so
  where the target already has the features it is inlined into `checksum`.
  Before, every CRC-32/ISCSI call on x86 and every CRC-32/ISCSI and
  CRC-32/ISO-HDLC call on aarch64 ran feature detection first, up to four
  atomic loads. On a Graviton2 (Neoverse N1, a c6g.xlarge on EC2,
  `aarch64-neon-pmull`), checksums matched 1.11.0 and the test suite passed;
  with the function pointer CRC-32/ISCSI and CRC-32/ISO-HDLC were 1.03x to
  1.35x faster from 1 to 512 bytes, aligned and unaligned (the closest, 200
  bytes, 1.00x), and the same from 4 KiB up. With the direct call and the
  aarch64 work below, the final source against 1.11.0 is in the aarch64
  table further down; on the Apple M5 Max an intermediate build of the
  direct call was 0.85x to 0.91x from 128 to 200 bytes, a gap the final
  source no longer has (128 bytes 2.52x to 2.60x).
- These changes come from reports by cemeyer, crc-turbo#3 (dispatch) and
  crc-turbo#4 (the Sierra Forest gap, with the measurements against
  fast-crc32's kernels that showed it).
- CRC-32/ISO-HDLC and the other folded algorithms are faster on x86 below
  4 KiB, on every tier. The methods are the Linux kernel's x86 CRC template
  and libdeflate's, reimplemented here from their descriptions with this
  crate's generated keys; no code is taken from either:
  - a 256-bit fold for buffers from 32 to 255 bytes, two 16-byte blocks per
    carry-less multiply, in place of one block per multiply;
  - one Barrett reduction at the end of every buffer under 1 MiB, where the
    fold reduced each 128-byte pass;
  - the tail blocks of a buffer folded independently towards the end, each
    with its own key, and XORed together, rather than one after another;
  - CRC-32/ISO-HDLC entries that take their keys as constants of the
    algorithm rather than loading them through the parameters.

  A test forces each x86 tier onto these paths over every algorithm, both
  bit orders, every length up to 64 bytes and from 120 to 400, lengths
  around 512 bytes, 1 KiB, 4 KiB and 8 KiB, four starting offsets and a
  carried state, against the software path; swapping one coefficient of the
  256-bit fold fails 34 tests. Measured on EC2 against 1.11.0, CRC-32/ISO-HDLC,
  nanoseconds per call, median of seven interleaved rounds, checksums
  compared first, from an aligned start:

  | bytes | EPYC 7R32 1.11.0 | 1.11.1 | EPYC 7R13 1.11.0 | 1.11.1 | Xeon 8375C 1.11.0 | 1.11.1 |
  |---:|---:|---:|---:|---:|---:|---:|
  | 16 | 20.8 | 5.8 | 15.7 | 5.3 | 16.8 | 4.6 |
  | 64 | 27.6 | 10.6 | 22.0 | 8.1 | 36.8 | 6.3 |
  | 128 | 31.2 | 16.4 | 27.4 | 10.6 | 30.5 | 8.2 |
  | 256 | 39.9 | 26.7 | 28.6 | 14.2 | 38.3 | 14.8 |
  | 256, unaligned | 67.5 | 26.7 | 62.9 | 14.2 | 92.1 | 14.4 |
  | 512 | 56.1 | 46.1 | 35.9 | 22.0 | 37.5 | 20.0 |
  | 1024 | 94.3 | 84.8 | 53.1 | 39.8 | 38.8 | 31.9 |
  | 4096 | 327 | 318 | 160 | 147 | 95.7 | 92.2 |

  From 16 KiB to 1 MiB the three CPUs measured 0.99x to 1.02x. The other
  folded algorithms were 0.99x to 3.89x across the same lengths. Of 17
  lengths from 16 bytes to 1 MiB, aligned and unaligned, CRC-32/ISCSI and
  CRC-32/ISO-HDLC read 0.99x or better against 1.11.0 everywhere but
  CRC-32/ISCSI at 2 KiB and 4 KiB on the 8375C, 0.96x to 0.97x: that length is bimodal on that CPU in both
  releases (43.4 or 40.5 GB/s), and a rerun of the final source read 1.00x.
  Against the fastest of fast-crc32's generated C, libdeflate and zlib built
  in the same binary, this release does not yet match them everywhere:
  CRC-32/ISO-HDLC from 32 to 512 bytes on the 7R32 (0.74x to 0.93x), 128 to
  192 bytes on the 7R13 (0.84x) and 512 bytes to 4 KiB on the 8375C against
  fast-crc32's AVX-512 kernel (0.76x to 0.93x); CRC-32/ISCSI from 16 to 48
  bytes on the 7R13 and the 8375C (0.81x to 0.88x) and at 1 MiB on the
  8375C (0.81x).
- The AVX-512 tiers are faster again for short and middling buffers, and
  for CRC-32/ISCSI on the CPUs where the stream kernel lost to the fold.
  The methods are those of the Linux kernel's x86 CRC template (Eric
  Biggers): the two 512-bit registers left by the main loop are reduced to
  128 bits in one step, each 128-bit lane multiplied by the generated key
  for its distance from the last, the keys read as memory operands; no
  kernel code is taken.
  - CRC-32/ISO-HDLC from 128 bytes to 4 KiB is folded from where it starts
    by a function of its own, apart from the short arms and from the long
    fold's set-up; from 128 bytes the `x86_64-avx512-vpclmulqdq` tier takes
    the first 128 bytes as two 512-bit registers.
  - From 64 KiB the folds first run to a 64-byte boundary. On a Xeon 8488C
    a buffer whose 64-byte loads split cache lines ran at 0.80x to 0.85x of
    an aligned one.
  - CRC-32/ISCSI on the `x86_64-avx512-vpclmulqdq` tier takes the 512-bit
    fold from 4 KiB up to 8 KiB (16 KiB on Ice Lake server parts), and on
    Ice Lake and AMD family 0x19 a new kernel from 16 KiB and 64 KiB: four
    512-bit folding streams beside five CRC32-instruction streams, the
    streams shifted together with multipliers built at compile time. On
    Sapphire Rapids that kernel ran at 0.77x of the existing one, so it is
    not selected there.
  - CRC-32/ISCSI on the `x86_64-avx512-pclmulqdq` tier (Skylake-X, Cascade
    Lake) uses the AVX2 `v4x2s4x4` kernel below 32 KiB.

  Against this branch's previous commit, GB/s, median of seven interleaved
  rounds on EC2 `.large` instances, checksums compared first: CRC-32/ISCSI
  on a Xeon 8275CL 1.11x to 1.92x from 512 bytes to 16 KiB; CRC-32/ISO-HDLC
  on a Xeon 8488C 1.25x to 1.38x from 256 bytes to 2 KiB, and 1.25x and
  1.22x at 64 KiB and 1 MiB from an unaligned start; on an EPYC 9R14 1.30x
  to 1.33x at 128, 256 and 512 bytes. The tiers that did not change, on a Xeon
  E5-2666 v3, an EPYC 7R32 and an EPYC 7R13, were 0.99x to 1.02x from
  512 bytes to 1 MiB. Below 0.97x: CRC-32/ISCSI from 32 to 128 bytes on the
  E5-2666 v3 and the 8275CL (0.91x to 0.96x, with that path's source
  unchanged), and CRC-32/ISO-HDLC on the 8275CL at 16 bytes (0.80x) and
  128 bytes (0.90x); neither is explained. Against 1.11.0 every size from
  16 bytes to 1 MiB on those six CPUs was 0.99x or better. An Ice Lake
  server part (Xeon 8375C) measured the changes up to the stream kernel's
  thresholds; the last revision of the 128-byte entry and the 64-byte
  alignment were not measured there.
- x86, after the change above, against it, measured on the same seven EC2
  `.large` CPUs (Xeon E5-2666 v3, EPYC 7R32, EPYC 7R13, Xeon 8275CL, Xeon
  8375C, Xeon 8488C, EPYC 9R14):
  - The generic fold's 32 to 255 bytes (`process_32_to_255`) are back in
    the form they had before the fold was refactored through a shared
    helper, which had changed the code every tier inlines for those
    lengths; on aarch64 the entries and folds compile to the same
    instructions as before it. The entries for buffers under 128 bytes and the folds of the
    `x86-sse-pclmulqdq`, `x86_64-avx2-vpclmulqdq` and
    `x86_64-avx512-pclmulqdq` tiers compile to the same instructions as
    before it again.
  - The `x86_64-avx512-pclmulqdq` tier no longer enables AVX512BW in its
    code (detection still requires it, as above): with it the compiler
    replaced byte blends with mask-register sequences. The
    `x86_64-avx512-vpclmulqdq` tier enables it in its folds only.
  - Only the `x86_64-avx512-vpclmulqdq` tier, whose fold loads 64 bytes at
    a time, takes a buffer from 64 KiB to a 64-byte boundary first; on the
    other tiers it was no faster from an unaligned start at 64 KiB and
    1 MiB and made their folds larger.
  - On the `x86_64-avx512-vpclmulqdq` tier, 192 to 255 bytes fold their
    first 64 bytes 128 bytes on, onto the third 64, before the one-step
    reduction, rather than folding the four blocks after the eighth in
    256-bit registers: CRC-32/ISO-HDLC at 192 bytes 1.40x faster on the
    8375C, 1.26x on the 9R14 and 1.06x on the 8488C, the other algorithms
    1.12x to 1.29x (one link layout).
  - CRC-32/ISO-HDLC from 128 bytes to 4 KiB loads its fold keys on the
    `x86-sse-pclmulqdq`, `x86_64-sse-pclmulqdq` and
    `x86_64-avx512-pclmulqdq` tiers. With the parameters a constant, the
    compiler built each 64-bit key in a general register and moved it into
    the vector unit, on the port the carry-less multiplies use. Loaded:
    1.14x to 1.15x at 128 and 192 bytes and 1.08x at 256 on the E5-2666 v3,
    1.05x to 1.12x from 128 to 256 bytes on the 8275CL; on the 7R32 0.96x
    to 1.04x. On the other two tiers it was 0.93x to 1.03x, so they keep the
    built keys, as do buffers under 128 bytes, where the 7R32, the 7R13,
    the 8375C and the 8488C lost up to 10%.
  - Timing below 512 bytes depends on where the linker places the code as
    much as on the code. Rebuilt with the same source and only the link
    order shuffled (lld `--shuffle-sections`), the same cell moved by a
    median 3% to 12% per CPU and up to 2.7x; 60% to 87% of the cells up to
    512 bytes differed by more than 1% between two pairs of such builds.
    A single build's comparison against another therefore repeats its
    layout's bias from run to run. The figures above are geometric means
    over four link layouts per build.
- x86, `x86_64-avx2-vpclmulqdq` and `x86_64-avx512-vpclmulqdq` tiers, every
  algorithm: a buffer of 256 bytes or more that is a whole number of
  128-byte blocks goes from the fold straight to the final reduction, as
  fast-crc32's generated `avx512_vpclmulqdq_crc32_v4` kernel does. It went
  through the step for the blocks left over, which with none left was an
  indirect jump on the count. CRC-32/ISO-HDLC at 256 bytes from an aligned
  start, against the fastest other library (fast-crc32's `v4`), GB/s,
  geometric mean of two interleaved rounds over four link layouts on EC2
  `.large` instances, checksums compared first: 1.05x on a Xeon 8488C and
  1.07x on an EPYC 9R14, where the same build without the change measured
  0.90x and 0.97x; from an unaligned start 1.25x on the 8488C, where it
  measured 1.04x. On a Xeon 8375C it was 1.45x with the change and 1.46x
  without. Against 1.11.0 at 256 bytes: 5.36x on the 8488C, 2.91x on the
  9R14, 3.96x on the 8375C. Lengths that are not a whole number of blocks
  take the path they took before.
- x86: the folds of 16 to 255 bytes on the `x86-sse-pclmulqdq` and
  `x86_64-sse-pclmulqdq` tiers, and of 16 to 127 bytes on the three wider
  tiers, are inline assembly, for every algorithm, so that a new LLVM
  cannot change their shape. Each length of 16-byte blocks has a
  straight-line arm folding each block onto the last across its own
  distance with the generated key for it, read from the key table; the
  bytes after the last whole block are folded in with one overlapping read.
  The wider tiers' 256-bit fold of 64 to 127 bytes, which LLVM had left as
  a call out of line, is no longer used there for the generic algorithms;
  CRC-32/ISO-HDLC has its own wide arms, below. Each fold sits in one function per tier, width and bit
  order, never inlined and not generic; Miri and 32-bit x86 run the intrinsics they are tested against.
  CRC-32/ISO-HDLC has an entry of its own per tier with the assembly in
  line, and on the SSE tier of a CPU with AVX takes the AVX encoding. A
  test checks each fold against the software path over every algorithm,
  every length it takes, four starting offsets and a carried
  state, and changing one key offset, multiply selector or blend operand
  fails it.
- x86, CRC-32/ISO-HDLC from 16 to 255 bytes takes libdeflate's shape for
  each register width (`crc32_pclmul_template.h`, reimplemented from its
  description; no code is taken). On the 128-bit tiers each fold's pair of
  multipliers is loaded into a register once and both multiplies read it,
  as libdeflate holds its multipliers in registers, instead of each
  multiply reading the pair from memory. The checksum entries of the
  `x86_64-avx2-vpclmulqdq` and `x86_64-avx512-vpclmulqdq` tiers fold 64 to
  255 and 64 to 127 bytes at their full width: the CRC and the first 32 or
  64 bytes in one register, every further 32 or 64 bytes folded onto the
  end across its distance, then 512 to 256 to 128 bits, with the bytes
  after the last whole block folded in by one overlapping read. Their keys
  are computed at compile time from the generated ones' powers of x,
  checked equal to them, in a 64-byte-aligned table. Tests check both
  widths against the software path from a carried state, every length to
  600 bytes and four starting offsets, and moving one key row in either
  width fails them. Measured on seven EC2 `.large` CPUs (E5-2666 v3,
  8275CL, EPYC 7R32, 7R13 and 9R14, 8375C and 8488C), median of two
  interleaved rounds over four link layouts, from an aligned and an
  unaligned start, 16 to 192 bytes: 2.24x to 10.15x 1.11.0's speed
  (geometric mean 5.10x). Against the fastest of fast-crc32's generated
  C, libdeflate and zlib: 0.86x to 1.82x (geometric mean 1.14x); the
  wide arms took the 7R13 from 0.85x to 0.97x at 80 to 192 bytes to 1.07x
  to 1.23x, and the 9R14 from 0.80x to 0.87x at 80 to 112 bytes to 1.16x
  to 1.30x. libdeflate still leads on the E5-2666 v3 from 32 to 96 bytes,
  at 128 from an unaligned start and at 192 (0.86x to 0.96x), the 8275CL
  from 32 to 96 bytes (0.87x to 0.96x) and the 7R32 from 80 to 112 bytes (0.93x to 0.96x). On those
  tiers its compiled path at 64 and 96 bytes runs the same number of
  multiplies and about the same number of instructions as this crate's,
  with a longer chain of dependent multiplies, and the E5-2666 v3's cells
  moved by up to 8% between two builds that differ only in code the CPU
  never runs.

- WebAssembly is faster for 43 of the 50 built-in algorithms. Every table
  load there is bounds-checked, so for a long input the software path now
  reduces the message by a sparse multiple of the generator first, in the
  manner of Sam Russell's Chorba (arXiv:2412.16398) for CRC-32/ISO-HDLC:
  when `x^n + x^(n - d_1) + ... + 1` is a multiple of the generator, each
  64-bit word is XORed `d` bits further on, for each offset `d`, with shifts
  in place of table lookups, and the last few words go through the tables.
  The offsets of each generator (three to five, all at least 64) are found by
  a search in `src/generate.rs` that a test checks the kernels against, and
  every kernel is checked against the tables over every algorithm, both bit
  orders, lengths from 0 to 4 KiB, unaligned starts and a carried state, on
  every target. Each algorithm's kernel is a constant of its calculator, and
  from which length it runs is set per generator and bit order from where it
  overtook the tables. Measured against 1.11.0 under wasmtime and Node on an
  Apple M-series Mac, `wasm32-wasip1`, nanoseconds per `checksum` call,
  median of five interleaved rounds, the slower of the two runtimes' ratios,
  checksums compared first for every algorithm from 0 to 1,100 bytes and
  larger, one-shot and split:

  | algorithm | 256 B | 1 KiB | 64 KiB |
  |---|---:|---:|---:|
  | CRC-16/DNP | 2.13x | 3.55x | 4.29x |
  | CRC-32/ISO-HDLC | 1.43x | 1.97x | 2.22x |
  | CRC-32/JAMCRC | 1.43x | 1.96x | 2.17x |
  | CRC-64/GO-ISO | 1.94x | 2.00x | 2.02x |
  | CRC-32/ISCSI | 1.36x | 1.62x | 1.73x |
  | CRC-32/AUTOSAR, BASE91-D, CD-ROM-EDC, MEF | 1.14x to 1.70x | 1.33x to 1.70x | 1.40x to 1.83x |
  | reflected CRC-16 (11 more) | 1.54x to 1.67x | 1.54x to 1.60x | 1.49x to 1.54x |
  | forward CRC-16 (19) | 0.98x to 1.22x | 1.15x to 1.56x | 1.19x to 1.92x |
  | CRC-32/BZIP2, CKSUM, MPEG-2 | 1.00x to 1.01x | 1.20x to 1.22x | 1.42x to 1.43x |
  | CRC-32/AIXM | 0.99x | 1.10x | 1.27x |

  No algorithm at any length from 32 bytes to 64 KiB was below 0.98x in
  either runtime, the seven algorithms without a kernel included, and below
  64 bytes every algorithm was 1.01x to 1.10x. Native targets keep the
  tables, which measured faster there.
- On those same targets each built-in algorithm has its own calculator,
  which links its algorithm's table alone: a `no_std` WebAssembly module
  computing CRC-64/NVME through `checksum`, `crc64_nvme` and `Digest`, built
  without LTO, is 38,267 bytes, from 718 KiB, and 56 KiB from 767 KiB with
  `std`. CI builds that module (`wasm-size/`) and fails above 64 KiB. Custom
  parameters without the `cache` feature built a table per call and leaked
  it; the table is now freed when the call returns. That table and the
  `generated-tables` ones are built in place on the heap, not as a 32 KiB
  stack temporary. From crc-fast#67 by Andres J Ruiz Torres; its
  slice-by-16 loop rewrite is not taken.
- A test forces each aarch64 tier, the SHA3 one on any CPU that has it, and
  checks every algorithm against the software path across the fold.
- CRC-32/ISCSI and CRC-32/ISO-HDLC are faster on aarch64 from 1 KiB, on
  the SHA3 tier. Over 1 KiB the fusion kernel runs three CRC32 instruction
  streams beside the PMULL fold and shifts them back together at the end,
  and worked out the three multipliers by repeated squaring on every call.
  They now come from tables built at compile time: one entry per 64-bit
  word up to 16 KiB, and one per 16 KiB step beyond that up to 16 MiB, two
  entries combined with a carry-less multiply and a CRC32 instruction; only
  a stream longer than 16 MiB still squares. A test checks every entry of
  both tables against the squaring, and another every length around the
  table boundaries against the software path; changing either table's
  starting power fails both. Measured against 1.11.0 on an Apple M5 Max,
  median of 15 interleaved rounds (21 from 128 to 512 KiB), checksums
  compared first for every algorithm:

  | bytes | CRC-32/ISCSI | CRC-32/ISO-HDLC |
  |---:|---:|---:|
  | 1025 | 2.23x | 2.26x |
  | 2048 | 2.07x | 2.07x |
  | 4096 | 1.85x | 1.84x |
  | 8192 | 1.48x | 1.42x |
  | 16384 | 1.22x | 1.26x |
  | 32768 | 1.12x | 1.16x |
  | 65536 | 1.03x | 1.11x |
  | 131072 | 1.05x | 1.05x |
  | 262144 | 1.03x | 1.02x |
  | 1048576 | 1.04x | 1.02x |

  Up to 1 KiB the kernel takes no shift. The PMULL tier without SHA3 and
  the other algorithms do not use this kernel.
- aarch64 off Apple, CRC-32/ISCSI and CRC-32/ISO-HDLC: the SHA3 tier runs
  the CRC32 instruction loop below 80 bytes, where the four-lane fold was
  slower at 64 bytes on Graviton 3 (9.2 to 10.4 GB/s against 12.8) and
  Graviton 4 (13.8 to 14.9 against 17.2 to 17.5). The PMULL tier, which
  Graviton 2 takes, runs the loop below 1 KiB and the four-stream kernel
  from 1 KiB, where the loop alone outran both lane folds at every length
  from 64 bytes and the stream kernel won from 1 KiB. The stream kernels
  finish their remainder with the loop. The SHA3 kernel's main loop issues
  all its multiplies before the three-way XORs again: XOR after each pair
  ran 0.94x to 0.96x of 1.11.0 on Graviton 3 at 1 MiB, this order 0.99x to
  1.02x. Below 64 bytes every tier calls a copy of the loop without its
  64-byte block step: with one copy called from three length ranges the
  compiler no longer specialised it, and from 16 to 48 bytes both
  algorithms ran 0.78x to 0.92x of the previous commit on Graviton 2, 3
  and 4. A test calls the short copies directly at every length below 64
  bytes from 16 start offsets with a carried state, and the dispatch test
  covers each new threshold on either side. Apple's paths are unchanged.
- aarch64 Linux and Android, CRC-32/ISCSI and CRC-32/ISO-HDLC: the CRC32
  instruction loops and the SHA3 tier's four-lane fold are now assembly in
  the crate (`crc32::fusion::aarch64::asm`), each one `asm!` block in a
  non-generic `#[inline(never)]` function with the C calling convention:
  the compiler adds only the return, and the block's opening `.p2align 6`
  raises the alignment of the function's own section, so the entry sits on
  a 64-byte boundary with no padding executed. Compiled, the short loop
  matched 2ee9ff1 instruction for instruction yet ran 0.90x to 0.93x of it
  from 16 to 48 bytes on Graviton 3 and 4, a cycle that moved with where the
  code sat; how a consumer's compiler and profile place compiled code is
  not this crate's to fix, so the hot code is no longer compiled. The SHA3
  tier runs the loop to 128 bytes on these targets, where it ran 1.17x to
  1.22x of the assembly fold from 80 to 112 bytes on Graviton 3 and 1.01x
  to 1.49x on Graviton 4 (aligned start; unaligned 0.99x to 1.18x). Tests
  call each assembly entry directly at every length to 1100 bytes from 16
  start offsets with a carried state; on Graviton 2, 3 and 4 a changed step
  bit or loop condition in the assembly failed them, and on Graviton 3 and
  4 a changed fold key. In two batches of seven interleaved rounds on
  Graviton 2, 3 and 4, from 16 bytes to 1 MiB, aligned and unaligned, both
  algorithms ran at or above 1.00x of 1.11.0 in every cell of the first
  batch (1.01x to 3.7x); in the second, one cell (Graviton 3, ISO-HDLC,
  64 KiB from an unaligned start) ran 0.96x. Against 2ee9ff1, Graviton 2
  was 0.96x to 1.67x; Graviton 3 ran 0.79x to 0.88x from 80 to 112 bytes
  from an unaligned start and 0.93x to 0.97x
  for some cells from 96 bytes to 1 KiB, and Graviton 4 0.89x to 0.98x for
  some cells from 96 bytes to 1 KiB, where the SHA3 tier's assembly fold
  ran slower than 2ee9ff1's compiled one. Built with Rust 1.101 beta and
  1.102 nightly on Graviton 3, CRC-32/ISCSI ran 0.96x to 1.05x of the
  pinned 1.97.1 build, but CRC-32/ISO-HDLC from 16 to 64 bytes ran 0.90x to
  0.92x: the dispatch around the assembly is still compiled. The other
  targets, Apple's included, keep the compiled code.
- aarch64, CRC-32/ISCSI and CRC-32/ISO-HDLC: each kernel takes the
  algorithm's final XOR and applies it before returning, the assembly ones
  with one more instruction (`eor w0, w0, w3`), the compiled ones in an
  out-of-line wrapper on Linux and Android. `checksum` naming either
  algorithm then compiles to a byte load, the length compares and a jump to
  the kernel, with no stack frame; fast-crc32's kernels, which apply their
  complement themselves, have the same shape. On Linux and Android the SHA3
  tier runs the four-lane fold from 96 bytes, where fast-crc32's `v4` wins
  on Graviton 4, rather than from 128. Against the best of fast-crc32's
  generated kernels, libdeflate, zlib and 1.11.0, median of seven
  interleaved rounds, checksums compared first: Graviton2 CRC-32/ISO-HDLC
  at 128 bytes ran 1.14x, Graviton3
  CRC-32/ISO-HDLC at 192 bytes 1.00x and at 112 bytes 1.06x (0.90x to
  0.94x with the loop to 128). Graviton2 CRC-32/ISCSI at 1 MiB ran 0.97x
  to 0.98x of fast-crc32's `v3s4x2e_v2` and 1.63x of 1.11.0. On Graviton4
  from 96 bytes to 1 KiB, two builds whose code is identical at a length
  differed by up to 29% in the same batch, more than any difference under
  test, so those lengths are not settled by these runs. A test checks every
  kernel's result with a final XOR against the reference across all
  lengths to 300 bytes and the fold thresholds; dropping the assembly's XOR
  failed it on Graviton 2, 3 and 4. On the Apple M5 Max, where the code
  shape is unchanged, both algorithms ran 1.07x to 3.2x of 1.11.0 from
  16 bytes to 1 MiB, aligned and unaligned.
- With that kernel, the direct call above and this release's aarch64 short paths,
  CRC-32/ISCSI and CRC-32/ISO-HDLC against 1.11.0 on Graviton, median of
  seven interleaved rounds, checksums compared first and the test suite
  passed on each, aligned start (unaligned in brackets where it differs by
  more than a tenth):

  | bytes | Graviton2 c6g | Graviton3 c7g | Graviton4 c8g |
  |---:|---:|---:|---:|
  | 16 | 2.07x to 2.28x (2.80x) | 1.64x (2.26x) | 2.00x (2.45x) |
  | 64 | 1.57x to 1.62x (2.79x) | 1.52x (2.42x) | 1.61x (2.54x) |
  | 128 | 1.90x to 2.04x | 2.17x to 2.23x | 2.09x to 2.33x |
  | 256 | 1.60x to 1.64x | 1.94x | 1.69x to 1.76x |
  | 1024 | 1.18x to 1.23x | 1.58x to 1.66x | 1.58x to 1.62x |
  | 4096 | 1.55x | 1.62x to 1.63x | 1.68x to 1.70x |
  | 16384 | 1.64x to 1.65x | 1.08x to 1.30x | 1.26x to 1.27x |
  | 65536 | 1.62x | 1.04x to 1.10x | 1.06x to 1.08x |
  | 1048576 | 1.64x | 0.95x to 0.98x | 1.00x to 1.02x |

  On the Apple M5 Max the final source was 1.15x to 3.47x at every length
  from 16 bytes to 1 MiB, aligned and unaligned. Graviton3 at 1 MiB,
  0.95x to 0.98x in that table, ran 0.98x to 1.04x of 1.11.0 once the SHA3
  kernel issued its multiplies before its three-way XORs again (above),
  and Graviton4 0.99x to 1.01x; the table is from before that change. The
  other algorithms' fold, in two batches of seven interleaved rounds, ran
  0.97x to 3.5x of 1.11.0 on Graviton2, 0.87x to 9.1x on Graviton3 and
  0.80x to 9.4x on Graviton4. Below 0.99x were the forward (not reflected)
  algorithms from 8 KiB to 1 MiB on Graviton2 (0.97x to 0.98x), where the
  fold's loop has 1.11.0's instructions in another order; on Graviton3
  2 KiB to 64 KiB and on Graviton4 8 KiB to 1 MiB, cells that differ by
  algorithm though every algorithm of a bit order runs the same fold, and
  that move by as much against 2ee9ff1, whose fold for these lengths is
  the same code. Taking an aligned buffer from 2 KiB through 1.11.0's
  aligned fold moved as many of those cells down as up, and aligning the
  loops did not close them; they are not explained. On the M5 Max 4 KiB,
  8 KiB and 128 KiB ran 0.94x to 0.96x in both bit orders, also not
  explained.
- The other algorithms' fold on aarch64 is back at or above 1.11.0 from
  2 KiB. The forward algorithms' loop reversed each 16-byte block with a
  `REV64` and an `EXT`, the compiler's rewrite of the `TBL` the source
  asks for, once it saw the mask was the constant byte reversal: 55
  instructions per 128 bytes, 48 of them vector, on Neoverse N1's two
  vector pipes (one of which runs PMULL). 1.11.0 has the same 55 and ran
  ahead only by the order the compiler gave them. The mask is now read
  through a volatile load, so each block takes one `TBL`: 47 instructions
  on the PMULL tier, 39 on the SHA3 tier. The fold for buffers under the
  cutoff ran its loop on an offset and a byte count, 41 instructions to
  the aligned loop's 39 (33 to 31 on SHA3), and now runs a pointer to the
  end as the aligned loop does. Off Apple the cutoff from that fold to the
  aligned one drops from 256 KiB to 64 KiB, where an unaligned start's
  loads split cache lines on Graviton3 and Graviton4 (see the
  `UNALIGNED_FOLD_END` notes); Apple keeps 256 KiB. Against 1.11.0 in the
  same process, all 50 algorithms, median of seven processes of seven
  interleaved rounds (five on Apple), checksums compared first, slowest algorithm and the
  range of geometric means from 2 KiB to 1 MiB, aligned start (three bytes
  off in brackets):

  | CPU | slowest | geometric mean |
  |---|---:|---:|
  | Graviton2 c6g | 1.00x (1.01x) | 1.10x to 1.17x (1.10x to 1.54x) |
  | Graviton3 c7g | 0.98x (1.00x) | 1.01x to 1.16x (1.02x to 2.31x) |
  | Graviton4 c8g | 0.98x (1.00x) | 1.11x to 1.22x (1.11x to 2.67x) |
  | Apple M5 Max | 0.99x (0.99x) | 1.12x to 1.16x (1.12x to 1.51x) |

  Before the change the forward algorithms ran 0.97x to 0.98x on Graviton2
  from 8 KiB, and 132 cells were under 0.99x there, 66 on Graviton4. Under
  0.99x now: on Graviton3 five reflected algorithms at 256 KiB from an
  aligned start (0.98x to 0.99x, where the code is unchanged and 1.11.0
  against a copy of itself in the same process ran as low as 0.98x), and on
  Graviton4 three cells at 32 KiB and 64 KiB (0.98x to 0.99x; that copy ran
  as low as 0.95x there). From 16 bytes to 1 KiB the slowest algorithm
  stays at 1.04x of 1.11.0 or above on every CPU here.

- On x86, CRC-32/ISCSI's short buffers are checksummed inside the caller.
  The plan's direct limit is kept in a static the entry reads with one
  relaxed load, and the CRC32 instruction over those buffers is written as
  inline assembly: a `#[target_feature]` function cannot be inlined into a
  caller built without SSE4.2, so the direct call above still cost a call,
  and the entry around it saved four registers for the paths that make the
  plan and fold. Those paths are now out of line, the kernel's call is
  inline, and the entry is never `None` on x86. Miri and 32-bit x86 keep the
  intrinsic loop.
- CRC-32/ISCSI's x86 hot paths now have a fixed shape whatever the compiler
  version decides, so a later LLVM cannot slow them by reshaping them:
  - the short path is a single inline assembly block: a 64-byte loop of
    eight CRC32 instructions, then a ladder of 32, 16, 8, 4, 2 and 1 byte
    steps in which a step not needed costs one branch not taken. A first
    version that tested the length's bits, a taken branch over each absent
    step, ran at 0.85x to 0.9x of the intrinsic loop at 64 and 128 bytes on
    a Xeon Platinum 8488C;
  - the entry takes the final XOR as a const parameter, so every arm ends
    in its own XOR and the long arm is a tail jump to an out-of-line plan
    function. Inlined into a caller that keeps the 32-bit result, the entry
    saves no registers and keeps no stack frame; widened to `u64`, as
    `checksum` returns it, only the long arm sets up a frame. The plan function's first-call and fold arms are tail
    jumps to their own out-of-line functions;
  - the AVX-512 VPCLMULQDQ kernel's `v3x2` shape aligns its start and
    finishes its last bytes in inline assembly. A change in the caller had
    let LLVM drop a range fact it had inferred, which grew the kernel from
    195 to 211 instructions. It is now 134 instructions, and a last fold of
    64-byte blocks at one 512-bit fold key replaces the 8-byte CRC32 loop
    over the remainder.

  Miri runs the intrinsic versions. Each assembly block has a test against
  the software path over every alignment within 16 bytes and lengths
  through three blocks, and a changed instruction in the block fails it.
- Past the CRC32 instruction's limit, a kernel of carry-less folds alone,
  fast-crc32's `v4` shape (four 128-bit accumulators, no CRC32 streams to
  shift back together), takes a window of lengths on the CPUs where it
  measured ahead of both the instruction and the plan's kernel, aligned and
  three bytes off:

  | CPU | CRC32 instruction through | `v4` through |
  |---|---:|---:|
  | Xeon E5-2666 v3 (Haswell) | 240 | 768 |
  | Xeon Platinum 8124M (Skylake-SP) | 240 | 768 |
  | EPYC 7R32 (Zen 2) | 288 | 447 |
  | EPYC 7R13 (Zen 3), AVX2 build | 320 | 447 |
  | Xeon Platinum 8488C (Sapphire Rapids) | 256 | 383; 447 off a 64-byte boundary |
  | Xeon Platinum 8375C (Ice Lake) | 256 | 383; 447 off a 64-byte boundary |
  | EPYC 9R14 (Zen 4) | 256 | 447 off a 64-byte boundary only |

  On the AVX-512 VPCLMULQDQ tier the 512-bit kernel stays ahead from a
  64-byte aligned start, where its loads do not split cache lines; from
  any other start `v4` was 1.17x to 1.5x faster at 320 and 384 bytes.
- On AMD family 0x17 (Zen 2), CRC-32/ISCSI from 32 KiB runs three CRC32
  instruction streams alone, fast-crc32's `s3` shape, with the stream
  shifts from the compile-time tables above. The EPYC 7R32 issues the CRC32
  instruction once a cycle and its carry-less multiply is slow beside it;
  the fused kernel was 0.94x to 0.98x of fast-crc32's `s3` there.
  Against 1.11.0, CRC-32/ISCSI on the seven x86 CPUs above, aligned and
  three bytes off, in two batches of seven interleaved rounds each:

  | CPU | 16 to 256 bytes | 512 bytes to 1 MiB |
  |---|---:|---:|
  | Xeon E5-2666 v3 (Haswell) | 1.45x to 3.29x | 1.01x to 2.79x |
  | Xeon Platinum 8124M (Skylake-SP) | 1.46x to 3.16x | 0.97x to 2.67x |
  | EPYC 7R32 (Zen 2) | 1.25x to 3.53x | 1.29x to 1.92x |
  | EPYC 7R13 (Zen 3) | 1.25x to 3.13x | 1.34x to 1.80x |
  | Xeon Platinum 8375C (Ice Lake) | 1.44x to 4.91x | 1.05x to 1.44x |
  | Xeon Platinum 8488C (Sapphire Rapids) | 1.25x to 4.67x | 0.88x to 1.41x |
  | EPYC 9R14 (Zen 4) | 1.32x to 3.73x | 1.00x to 1.19x |

  No cell was below 0.99x in both batches. The lows came in one batch
  each: Sapphire Rapids three bytes off a boundary at 8 KiB to 32 KiB
  (0.88x to 0.97x, 1.00x and above in the other batch, where fast-crc32's
  AVX-512 kernels in the same binary moved as much), and Skylake-SP at
  1 MiB three bytes off (0.97x). Below 0.99x of fast-crc32's best kernel
  in both batches remain 16 to 48 bytes aligned on Haswell and Skylake-SP
  (0.94x to 0.96x, the call through the algorithm's dispatch), 128 to 256
  bytes aligned on Sapphire Rapids (0.96x to 0.98x, where the CRC32
  instruction runs at its 8 bytes a cycle and fast-crc32's `v4` runs above
  it), and 512 to 2048 bytes on Zen 4 (0.96x to 0.99x, the fold).
- A `no_std` build for x86_64 chose the AVX-512 tiers from its compile-time
  target features on AVX512VL alone. Those kernels also use AVX512BW, so a
  target that enables AVX512VL without AVX512BW, as for a guest whose
  hypervisor masks it, reached instructions the CPU may not have. It now
  takes the tiers below, as `std` builds already did; both detection paths
  decide through one rule. `std` builds are unchanged. The gap is item 3 of
  crc-fast#64 by GiorgioRegni, which 1.11.0 closed for `std` builds only.

- The compiled shape of the hot paths is now checked, not assumed. `cargo
  xtask codegen check` reads the assembly the compiler emitted for each
  function listed in `codegen/<target>.snap` and fails when a listed
  function's inline assembly differs, when the code generated around it grows
  past the snapshot's margin, or when it gains a call. It runs in CI on the
  pinned toolchain (exact), on the MSRV and under the profile a dependent
  crate gets by default, and weekly on beta and nightly as an early warning.
  It counts instructions and never times anything.

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