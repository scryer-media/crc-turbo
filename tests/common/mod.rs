// Copyright 2026 the crc-turbo authors. Licensed under MIT or Apache-2.0.

//! Spawning this crate's own binaries from a test.

use std::process::Command;

/// A command for one of this crate's binaries, given its `CARGO_BIN_EXE_*` path.
///
/// Cargo starts the test binary through the target's runner when one is configured, but the
/// test's own children get no such help: under `cross test` the test process runs in QEMU and
/// a child of the foreign architecture cannot be executed directly. The runner reaches the
/// test's environment as `CARGO_TARGET_<TRIPLE>_RUNNER` (cross's images export it), so when the
/// variable for this architecture is set, the binary is run through it.
pub fn bin(path: &str) -> Command {
    let prefix = format!(
        "CARGO_TARGET_{}_",
        std::env::consts::ARCH.to_ascii_uppercase()
    );
    let runner = std::env::vars()
        .find(|(name, _)| name.starts_with(&prefix) && name.ends_with("_RUNNER"))
        .map(|(_, value)| value);
    let mut words = runner.as_deref().unwrap_or_default().split_whitespace();
    match words.next() {
        Some(runner) => {
            let mut command = Command::new(runner);
            command.args(words).arg(path);
            command
        }
        None => Command::new(path),
    }
}
