# Publishing

Releases are published by `.github/workflows/release.yml`, started one of two
ways:

- **From GitHub Actions** (the usual way): run the `release` workflow on
  `main`. It releases the version `main`'s `Cargo.toml` names; nothing is
  typed in. No local signing or tagging is involved; the workflow creates
  the tag. See "Each release" below.
- **From a machine**, with `cargo xtask release`, which pushes a signed tag
  that starts the workflow. The task (`xtask/src/main.rs`, plain Rust with no
  dependencies, so it runs wherever `cargo` does) runs the local checks the
  workflow's verify job also runs (branch, clean signed tree, changelog and
  README sections, `cargo test`, `cargo publish --dry-run`); the workflow
  adds `cargo semver-checks` and the ci-complete check on the commit.

Only the `crc-turbo` crate is published. The xtask, the fuzz harness and the C
library package (`capi/`) are `publish = false`.

## One-time setup, before the first release

1. **The crate has to exist on crates.io.** crates.io only lets a workflow
   publish a crate that already exists. `crc-turbo` does: version `0.0.0` is a
   placeholder that reserves the name, so no release has to be uploaded from
   a machine. (Had it not, `cargo xtask release --publish` would run the
   checks, create the signed tag, run `cargo publish` from the machine and
   then push the tag; the workflow sees the version on crates.io, skips its
   own upload and creates the GitHub release.) The placeholder is also the
   baseline `cargo semver-checks` compares the first release against, which
   it passes as a major bump.
2. **Turn on trusted publishing** at
   `https://crates.io/crates/crc-turbo/settings/new-trusted-publisher`:
   repository owner `scryer-media`, repository `crc-turbo`, workflow
   `release.yml`, environment `crates-io`.
3. **Create the `crates-io` environment** in the GitHub repository settings.
   If it is restricted to refs, allow both the branch `main` (dispatched
   releases) and the tags `v*` (pushed tags). No secrets are needed, the
   workflow gets a short-lived token from crates.io through OIDC.

The release workflow needs `contents: write` to create the GitHub release,
which it requests for that job only.

## Each release

1. On `main`:
   - set `version` in `Cargo.toml` and refresh `Cargo.lock`;
   - make sure `CHANGELOG.md` has a `## <version>` heading that is not marked
     unreleased (the `## Fork` section stays where it is; it is the record of
     the divergence from crc-fast, not a release note);
   - make sure the README's dependency line names the version or its
     major.minor;
   - commit, signed, and push. Wait for `ci` to go green on that commit.
2. Actions → `release` → Run workflow, branch `main`, `dry_run` left on.
   The version is the one in `Cargo.toml` on `main`. The run refuses a
   branch other than `main`, a changelog section for that version missing
   or still marked unreleased, a tag `v<version>` at another commit, a
   commit whose
   `ci-complete` is not green, a public API change larger than the version
   bump (`cargo semver-checks`, once a previous version is on crates.io),
   and a crate that does not package.
3. Run it again with `dry_run` off. It publishes to crates.io, then creates
   the tag `v<version>` at the commit it checked and the GitHub release with
   the changelog section as its notes. A run that fails after the upload can
   be rerun: a version already on crates.io, a tag already at that commit and
   a release that already exists are all taken as done.

From a machine instead, after step 1: `cargo xtask release --dry-run`, then
`cargo xtask release`. The dry run works on any branch and on a dirty tree,
and lists everything a real run would refuse rather than stopping at the
first. A real run refuses a dirty tree, an unsigned HEAD, a HEAD that is not on
`origin/main`, a branch other than `main`, a changelog section missing or
still marked unreleased, a README that
shows neither the version nor its major.minor, and a tag that already exists
somewhere other than HEAD. It then runs the tests in release mode
(`--skip-tests` leaves them to CI and to the workflow's `verify` job; not
with `--publish`, which needs them here) and a
`cargo publish --dry-run`, creates the signed tag `v<version>` and pushes it.

The workflow then verifies the tag against the manifest, tests, publishes to
crates.io and creates the GitHub release with the changelog section as its
notes.

Upstream's tags (`1.0.0` … `1.10.0`, no `v`) are crc-fast's releases and
stay in the history as they are; this crate's tags are `v1.11.0` onwards.
