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
crc-fast's open pull requests credits its author below and in the commit
that carries it. This section is the record of how the crate differs from
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
  on `main` (it creates the tag) or started by the signed `v<version>` tag
  `cargo xtask release` pushes; crates.io trusted publishing either way. See
  `docs/publishing.md`. Upstream's release workflow, which also built and
  attested binary packages of the C library and the CLI tools, is not
  carried; the `Makefile` builds and installs both from a checkout.
- Repository hygiene adopted from the scryer-media house style: SHA-pinned CI
  ending in a required `ci-complete` job that keeps all of upstream's lanes
  (aarch64, x86_64, 32-bit x86, the software fallback on big-endian PowerPC,
  Miri, `no_std`, WASM and a fuzz smoke of every target) and adds `clippy -D
  warnings` across feature sets, docs, MSRV and packaging; `security`
  (cargo-audit over both lockfiles, zizmor); `codeql`; gitleaks pre-commit
  hooks; renovate; issue templates; `AGENTS.md`, `SECURITY.md`,
  `CONTRIBUTORS.md`. Upstream's `tests.yml` and `release.yml` were removed
  as duplicates of these.
- The crate archive no longer carries the repository's CI, hook, fuzz and
  agent files, nor upstream's `.kiro/` design notes, which stay in the
  repository.

## 1.11.0

The first release of the fork. It is everything in the [Fork](#fork) section
above.

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