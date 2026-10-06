//! `cargo xtask pre-commit`: what `.githooks/pre-commit` runs. Rejects staged
//! lines that name the local user or home directory, then has `gitleaks` look
//! at the staged changes for secrets.

use std::{env, process::ExitCode};

use crate::cmd::{capture, capture_all, repo_root};

pub fn pre_commit() -> ExitCode {
    env::set_current_dir(repo_root()).expect("enter the repository root");
    if personal_references() && secrets() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// True when no added line names the local user or home directory.
fn personal_references() -> bool {
    let user = ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|name| env::var(name).ok())
        .filter(|user| !user.is_empty())
        .map(|user| user.to_lowercase());
    let home = ["HOME", "USERPROFILE"]
        .iter()
        .find_map(|name| env::var(name).ok())
        .filter(|home| !home.is_empty());
    if user.is_none() && home.is_none() {
        return true;
    }

    let diff = capture(
        "git",
        &[
            "diff",
            "--cached",
            "--no-color",
            "--unified=0",
            "--diff-filter=ACMR",
            "--",
            ".",
        ],
    )
    .unwrap_or_default();

    let mut file = "unknown";
    let mut hits = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = path;
        } else if let Some(added) = line.strip_prefix('+')
            && !line.starts_with("+++")
        {
            let names_user = user
                .as_deref()
                .is_some_and(|user| added.to_lowercase().contains(user));
            let names_home = home.as_deref().is_some_and(|home| added.contains(home));
            if names_user || names_home {
                hits.push(format!("  - {file}:{added}"));
            }
        }
    }
    if hits.is_empty() {
        return true;
    }
    eprintln!(
        "pre-commit blocked: staged changes contain local personal path or username references."
    );
    eprintln!("Remove or generalize these lines before committing:");
    for hit in hits {
        eprintln!("{hit}");
    }
    false
}

/// True when `gitleaks` finds nothing in the staged changes.
fn secrets() -> bool {
    if capture_all("gitleaks", &["version"]).is_none() {
        eprintln!("pre-commit blocked: gitleaks is required for commits in this repo.");
        eprintln!("Install gitleaks, ensure it is on PATH, then rerun the commit.");
        return false;
    }
    let config = format!("--config={}", repo_root().join(".gitleaks.toml").display());

    let (mut clean, mut output) =
        capture_all("gitleaks", &["protect", "--staged", "--redact", &config])
            .expect("gitleaks ran a moment ago");
    // Newer gitleaks dropped `protect` for `git --staged`.
    let unknown_command = [
        "unknown command",
        "unknown flag",
        "unexpected argument",
        "help for",
    ];
    let lower = output.to_lowercase();
    if !clean && unknown_command.iter().any(|text| lower.contains(text)) {
        (clean, output) = capture_all("gitleaks", &["git", ".", "--staged", "--redact", &config])
            .expect("gitleaks ran a moment ago");
    }
    if !clean {
        eprintln!("pre-commit blocked: gitleaks found possible secrets in staged changes.");
        for line in output.lines() {
            eprintln!("  {line}");
        }
    }
    clean
}
