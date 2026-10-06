//! Repository tasks, run as `cargo xtask <task>`. Plain Rust with no
//! dependencies, so every task runs wherever `cargo` does.

mod cmd;
mod hooks;
mod release;

use std::{env, process::ExitCode};

const TASKS: &str = "\
cargo xtask release     cut a release; see `cargo xtask release --help`
cargo xtask pre-commit  what .githooks/pre-commit runs";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("release") => release::release(args),
        Some("pre-commit") => hooks::pre_commit(),
        Some("-h" | "--help") | None => {
            println!("{TASKS}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("xtask: unknown task '{other}'\n\n{TASKS}");
            ExitCode::from(2)
        }
    }
}
