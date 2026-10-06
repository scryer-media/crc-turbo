# Agent and contributor rules for crc-turbo

These rules apply to every automated agent and every human contributor.

## What this repository is

`crc-turbo` began as a fork of
[crc-fast](https://github.com/awesomized/crc-fast-rust) by Don MacAskill,
MIT OR Apache-2.0, taken at its 1.10.0 release (`3a853cc`). It is a hard
fork: it is not rebased onto crc-fast and is not kept in step with it. The
origin is recorded for licensing and for readers arriving from the other
crate, not as a constraint. The fork exists because crc-fast stopped taking
changes: its open pull requests (security bumps, `no_std` fixes, `digest`
0.11, a 256-bit x86 tier) had no answer, and the consumers of this crate
were carrying their own CRC code to work around the gaps.

The public Rust API started as crc-fast 1.10.0's, so a consumer's migration
is `crc_fast::` → `crc_turbo::` plus the dependency line. That compatibility
is kept: an addition is fine, a breaking change to an existing item needs a
major version and a reason, not convenience.

## Divergence rules

1. **Every change is ours to make.** There is no upstream to confine a diff
   for. Change what the work needs, where it lives.
2. **Bugs are fixed here.** A bug inherited from crc-fast is a bug in this
   crate; it is fixed and released here whether or not crc-fast has it too.
3. **Credit what is taken.** A change taken from a crc-fast pull request or
   issue names its author in the commit message body, in `CHANGELOG.md` and
   in `CONTRIBUTORS.md`.
4. **Keep the `## Fork` section of `CHANGELOG.md` honest.** It is the record
   of how this crate differs from the release it was taken at. A change that
   adds to that difference is noted there in the same commit.

## Kernel rules

- **The software path is the oracle.** Every accelerated tier, on every
  architecture, is checked against `arch::software::update` over every entry
  of the algorithm table (`TEST_ALL_CONFIGS`), both bit orders, a spread of
  lengths around the fold and block boundaries, unaligned starts and a
  carried state. A new tier lands with that test, forced onto the tier
  directly so it runs on any CPU that has the features, not only on CPUs
  where dispatch would pick it.
- **No hand-written constants.** Fold keys come from `src/generate.rs`; a
  kernel that needs a coefficient uses or derives one of the generated keys
  rather than pasting a literal. A mutation check (swap the coefficient,
  watch the test fail) is how a new kernel proves its test pins it.
- **Dispatch never regresses a tier it did not mean to touch.** A change to
  `feature_detection.rs` comes with selection tests for the CPUs on either
  side of it, and with a benchmark of the tiers it should not move.
- `unsafe` is confined to the architecture modules, the C library
  (`src/ffi.rs`) and the places upstream already has it. Every block calling
  an intrinsic is reached only through a function with the matching
  `#[target_feature]`, after detection.
- Miri runs the whole suite on x86_64. A change that makes a test
  Miri-incompatible ignores that one test under `cfg(miri)`, never the lane.

## Performance rules

- A change that claims a speed-up shows it against the previous release on
  the hardware it targets, median of interleaved rounds, with the checksums
  compared before timing. A change to one tier shows the others unchanged.
- The CPUs without the feature the change needs are part of the claim: an
  AVX2 change is measured on an AVX-512 part too, and aarch64 is measured
  whenever shared code moves.

## Tests

- A test gives the same result on a slow, loaded or shared runner as on an
  idle workstation. No fixed sleeps, no deadlines, no elapsed-time
  assertions, no timeouts as a stand-in for a condition.

## Repository hygiene

- Commits are SSH-signed. Never pass `--no-gpg-sign`.
- Never run destructive git working-tree operations (`checkout --`,
  `restore`, `reset --hard`, `clean`, `stash`) in a shared checkout.
- Branch names use gitflow prefixes: `feature/…`, `bugfix/…`, `hotfix/…`.
- Do not push. The maintainer pushes.
- The pre-commit hook (`.githooks/pre-commit`) rejects home paths, the local
  username and secrets. Keep it enabled: `git config core.hooksPath .githooks`.
- `cargo fmt --all`, `cargo clippy --locked --all-targets --all-features --
  -D warnings` and `cargo test --locked --all-features` must pass before a
  commit is proposed.
- The C header `libcrc_turbo.h` is generated: `test_ffi_header` rewrites it
  when it is stale and fails, and the rewritten file is committed.
- Any code change bumps the crate version, `Cargo.lock` and `CHANGELOG.md` in
  the same change.
