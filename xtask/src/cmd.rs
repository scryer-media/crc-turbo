//! Running other programs, and finding the repository.

use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one level under the repository root")
        .to_path_buf()
}

pub fn read(root: &Path, name: &str) -> String {
    std::fs::read_to_string(root.join(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

pub fn cargo(args: &[&str]) -> bool {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(&cargo, args)
}

/// Runs a command with its output on the terminal.
pub fn run(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

/// Runs a command silently, for its exit status alone.
pub fn succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Runs a command for its standard output, trimmed.
pub fn capture(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned()
    })
}

/// Runs a command for whether it succeeded and everything it printed, or
/// `None` when it could not be started.
pub fn capture_all(program: &str, args: &[&str]) -> Option<(bool, String)> {
    let output = Command::new(program).args(args).output().ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Some((output.status.success(), text))
}
