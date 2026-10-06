//! `cargo xtask release`: run the checks the release workflow runs, create the
//! signed tag and push it. `.github/workflows/release.yml` publishes to
//! crates.io and creates the GitHub release when the tag arrives. The same
//! workflow can also be dispatched from the Actions tab, which needs no local
//! tag at all; see `docs/publishing.md`.

use std::{env, process::ExitCode};

use crate::cmd::{capture, capture_all, cargo, read, repo_root, run, succeeds};

/// The crate this repository publishes.
const CRATE: &str = "crc-turbo";
const CHANGELOG: &str = "CHANGELOG.md";
const README: &str = "README.md";

const USAGE: &str = "\
cargo xtask release [--dry-run] [--publish] [--skip-tests]

  --dry-run     report every problem, change nothing; works on any branch and
                on a dirty tree
  --publish     after tagging, `cargo publish` from this machine, then push
                the tag (the first release, before trusted publishing exists)
  --skip-tests  leave out `cargo test`; CI on the release commit and the
                workflow's own verify job both run it";

struct Release {
    dry_run: bool,
    problems: u32,
}

impl Release {
    /// In a dry run a problem is reported and the run goes on; otherwise it
    /// is fatal.
    fn problem(&mut self, message: &str) -> Result<(), ()> {
        eprintln!("release: {message}");
        self.problems += 1;
        if self.dry_run { Ok(()) } else { Err(()) }
    }
}

pub fn release(args: impl Iterator<Item = String>) -> ExitCode {
    let (mut dry_run, mut publish, mut skip_tests) = (false, false, false);
    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--publish" => publish = true,
            "--skip-tests" => skip_tests = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("release: unknown option '{other}'\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let mut run = Release {
        dry_run,
        problems: 0,
    };
    match run.cut(publish, skip_tests) {
        Ok(()) if run.problems == 0 => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

impl Release {
    fn cut(&mut self, publish: bool, skip_tests: bool) -> Result<(), ()> {
        let root = repo_root();
        env::set_current_dir(&root).expect("enter the repository root");

        let Some(version) = crate_version() else {
            eprintln!("release: `cargo pkgid -p {CRATE}` did not name a version");
            return Err(());
        };
        let minor = version
            .rsplit_once('.')
            .map_or(version.as_str(), |(minor, _)| minor);
        let tag = format!("v{version}");
        println!("release: {CRATE} {version}");

        let branch = capture("git", &["branch", "--show-current"]).unwrap_or_default();
        if branch != "main" {
            self.problem(&format!("on '{branch}', releases are cut from main"))?;
        }
        let dirty = capture("git", &["status", "--porcelain"]).unwrap_or_default();
        if !dirty.is_empty() {
            self.problem(&format!("the tree is not clean:\n{dirty}"))?;
        }
        if !succeeds("git", &["verify-commit", "HEAD"]) {
            self.problem("HEAD is not a signed commit")?;
        }
        // A release that got as far as the tag and then failed - a refused
        // publish, a refused push - is picked up where it stopped, as long as
        // the tag is the signed tag this run would have made.
        let tagged = succeeds(
            "git",
            &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
        );
        if tagged {
            let at = capture("git", &["rev-parse", &format!("{tag}^{{commit}}")]);
            let head = capture("git", &["rev-parse", "HEAD"]);
            if at.is_none() || at != head {
                self.problem(&format!(
                    "tag {tag} exists and is not at HEAD; if it was never pushed, `git tag -d {tag}` and run again"
                ))?;
            } else if !succeeds("git", &["verify-tag", &tag]) {
                self.problem(&format!("tag {tag} exists and is not a signed tag"))?;
            } else {
                println!("release: tag {tag} is already at HEAD, carrying on from it");
            }
        }

        let changelog = read(&root, CHANGELOG);
        match changelog
            .lines()
            .find(|line| is_version_heading(line, &version))
        {
            None => self.problem(&format!("{CHANGELOG} has no '## {version}' section"))?,
            Some(heading) if heading.to_lowercase().contains("unreleased") => self.problem(
                &format!("{CHANGELOG} still calls {version} unreleased: '{heading}'"),
            )?,
            Some(_) => {}
        }

        // The README's dependency lines may name the full version or just
        // major.minor.
        let readme = read(&root, README);
        let shown = [&version[..], minor].iter().any(|v| {
            readme.lines().any(|line| {
                line.starts_with(&format!("{CRATE} = \"{v}\""))
                    || line.starts_with(&format!("{CRATE} = {{ version = \"{v}\""))
            })
        });
        if !shown {
            self.problem(&format!(
                "{README} shows neither \"{version}\" nor \"{minor}\" in its dependency lines"
            ))?;
        }

        if skip_tests {
            println!("release: skipping cargo test");
        } else {
            println!("release: cargo test --locked --workspace --all-features --release");
            if !cargo(&[
                "test",
                "--locked",
                "--workspace",
                "--all-features",
                "--release",
                "--no-fail-fast",
            ]) {
                self.problem("tests failed")?;
            }
        }

        println!("release: cargo publish --dry-run");
        let mut package = vec!["publish", "-p", CRATE, "--locked", "--dry-run"];
        if self.dry_run && !dirty.is_empty() {
            package.push("--allow-dirty");
        }
        if !cargo(&package) {
            self.problem("cargo publish --dry-run failed")?;
        }

        if self.dry_run {
            if self.problems == 0 && tagged {
                println!("release: dry run clean; a real run would carry on from {tag}");
            } else if self.problems == 0 {
                println!("release: dry run clean; a real run would tag {tag}");
            } else {
                eprintln!("release: dry run found {} problem(s)", self.problems);
            }
            return Ok(());
        }

        let message = format!("{CRATE} {version}");
        if !tagged && !run("git", &["tag", "-s", &tag, "-m", &message]) {
            return self.problem(&format!("could not create the signed tag {tag}"));
        }
        if publish {
            println!("release: cargo publish");
            let cargo_bin = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            let (ok, output) = capture_all(&cargo_bin, &["publish", "-p", CRATE, "--locked"])
                .unwrap_or((false, String::from("could not run cargo")));
            print!("{output}");
            if !ok && output.contains("already exists") {
                println!("release: {version} is already on crates.io, carrying on");
            } else if !ok {
                return self.problem(&format!(
                    "cargo publish failed; the tag {tag} exists locally and was not pushed. Fix the cause and run this again: it carries on from the tag"
                ));
            }
        }
        if !run("git", &["push", "origin", &tag]) {
            return self.problem(&format!("could not push {tag}"));
        }
        if publish {
            println!(
                "release: published {version} and pushed {tag}; the workflow finds it on crates.io and only creates the GitHub release"
            );
        } else {
            println!("release: pushed {tag}; the release workflow publishes it");
        }
        Ok(())
    }
}

/// `## 0.3.0`, `## 0.3.0 - 2026-09-17` or `## [0.3.0]`, but not `## 0.3.01`.
fn is_version_heading(line: &str, version: &str) -> bool {
    let Some(rest) = line.strip_prefix("## ") else {
        return false;
    };
    let rest = rest.strip_prefix('[').unwrap_or(rest);
    rest.strip_prefix(version).is_some_and(|tail| {
        !tail
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == '.')
    })
}

fn crate_version() -> Option<String> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let id = capture(&cargo, &["pkgid", "-p", CRATE])?;
    let version = id.rsplit(['#', '@']).next()?;
    Some(version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::is_version_heading;

    #[test]
    fn a_heading_names_exactly_its_version() {
        assert!(is_version_heading("## 0.3.0", "0.3.0"));
        assert!(is_version_heading("## 0.3.0 - 2026-09-17", "0.3.0"));
        assert!(is_version_heading("## [0.3.0] - 2026-09-17", "0.3.0"));
        assert!(is_version_heading("## 0.3.0 (unreleased)", "0.3.0"));
        assert!(!is_version_heading("## 0.3.01", "0.3.0"));
        assert!(!is_version_heading("## 0.3.0.1", "0.3.0"));
        assert!(!is_version_heading("### 0.3.0", "0.3.0"));
        assert!(!is_version_heading("## Fork", "0.3.0"));
    }
}
