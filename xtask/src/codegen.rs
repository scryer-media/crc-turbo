//! `cargo xtask codegen`: what the compiler made of the hot paths.
//!
//! A consumer builds this crate with their own compiler and profile, so the
//! instructions on a hot path are only the same for everyone where the path is
//! `asm!`. This task reads the assembly `rustc --emit asm` writes and, for each
//! function named in `codegen/<target>.snap`, records three things:
//!
//! - `asm`: a hash of the mnemonics between the inline-assembly markers, and
//!   how many there are. This is the part every consumer gets as written, so
//!   it must match the snapshot on every toolchain and profile.
//! - `glue`: how many instructions the compiler generated around it.
//! - `calls`: how many of those are calls.
//!
//! On the pinned toolchain and the release profile `glue` and `calls` must
//! match exactly, so a change that moves them is seen in review. On any other
//! toolchain or profile they may not grow past a small margin: that is the
//! early warning that a compiler has started treating the glue differently.
//! Nothing here measures time.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

use crate::cmd::{capture, read, repo_root};

const USAGE: &str = "\
cargo xtask codegen check [--target <triple>] [--profile release|consumer]
cargo xtask codegen bless [--target <triple>]

check  compare the functions in codegen/<target>.snap with a fresh build
bless  rewrite the numbers in codegen/<target>.snap from a fresh build
       (pinned toolchain and release profile only)

--profile consumer builds as a dependent crate would by default: no LTO,
sixteen codegen units, opt-level 2.";

/// How far the compiler's own instructions may grow off the pinned toolchain
/// or profile before the check fails: a tenth, plus four.
fn glue_limit(pinned: usize) -> usize {
    pinned + pinned / 10 + 4
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Shape {
    asm_hash: u64,
    asm: usize,
    glue: usize,
    calls: usize,
}

impl Shape {
    fn line(&self, path: &str) -> String {
        format!(
            "{path} asm={:016x}/{} glue={} calls={}",
            self.asm_hash, self.asm, self.glue, self.calls
        )
    }
}

pub fn codegen(mut args: impl Iterator<Item = String>) -> ExitCode {
    let task = args.next();
    let mut target = None;
    let mut profile = "release".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => target = args.next(),
            "--profile" => profile = args.next().unwrap_or_default(),
            _ => {
                eprintln!("xtask codegen: unknown argument '{arg}'\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    if !matches!(profile.as_str(), "release" | "consumer") {
        eprintln!("xtask codegen: unknown profile '{profile}'\n\n{USAGE}");
        return ExitCode::from(2);
    }
    let root = repo_root();
    let Some(target) = target.or_else(host_triple) else {
        eprintln!("xtask codegen: cannot tell the host triple; pass --target");
        return ExitCode::FAILURE;
    };
    let snap_path = root.join("codegen").join(format!("{target}.snap"));
    let Ok(snap_text) = fs::read_to_string(&snap_path) else {
        eprintln!("xtask codegen: no snapshot at {}", snap_path.display());
        return ExitCode::FAILURE;
    };
    let pinned = on_pinned_toolchain(&root);
    match task.as_deref() {
        Some("check") => {
            let Some(found) = build_and_measure(&root, &target, &profile, &snap_text) else {
                return ExitCode::FAILURE;
            };
            check(&snap_text, &found, pinned && profile == "release")
        }
        Some("bless") => {
            if !pinned || profile != "release" {
                eprintln!(
                    "xtask codegen: bless only on the pinned toolchain and the release profile"
                );
                return ExitCode::FAILURE;
            }
            let Some(found) = build_and_measure(&root, &target, &profile, &snap_text) else {
                return ExitCode::FAILURE;
            };
            bless(&snap_path, &snap_text, &found)
        }
        _ => {
            println!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn host_triple() -> Option<String> {
    let version = capture("rustc", &["-vV"])?;
    version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
}

/// Whether the compiler that will build is the one `rust-toolchain.toml` names.
fn on_pinned_toolchain(root: &Path) -> bool {
    let pinned = read(root, "rust-toolchain.toml");
    let Some(channel) = pinned
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .map(|value| value.trim_matches('"').to_owned())
    else {
        return false;
    };
    capture("rustc", &["--version"])
        .is_some_and(|version| version.split(' ').nth(1) == Some(channel.as_str()))
}

/// The paths a snapshot names, in order, with what it records for each.
fn parse_snapshot(text: &str) -> Vec<(String, Shape)> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(path) = words.next() else { continue };
        let mut shape = Shape::default();
        for word in words {
            if let Some(value) = word.strip_prefix("asm=") {
                if let Some((hash, count)) = value.split_once('/') {
                    shape.asm_hash = u64::from_str_radix(hash, 16).unwrap_or(0);
                    shape.asm = count.parse().unwrap_or(0);
                }
            } else if let Some(value) = word.strip_prefix("glue=") {
                shape.glue = value.parse().unwrap_or(0);
            } else if let Some(value) = word.strip_prefix("calls=") {
                shape.calls = value.parse().unwrap_or(0);
            }
        }
        entries.push((path.to_owned(), shape));
    }
    entries
}

fn build_and_measure(
    root: &Path,
    target: &str,
    profile: &str,
    snap_text: &str,
) -> Option<BTreeMap<String, Vec<Shape>>> {
    let out: PathBuf = root
        .join("target")
        .join("codegen")
        .join(format!("{target}-{profile}"));
    let deps = out.join(target).join("release").join("deps");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut build = Command::new(cargo);
    build
        .current_dir(root)
        .args([
            "rustc",
            "--message-format=json-render-diagnostics",
            "--locked",
            "--release",
            "--lib",
            "--target",
            target,
        ])
        .arg("--target-dir")
        .arg(&out)
        .args(["--", "--emit", "asm"]);
    if profile == "consumer" {
        build
            .env("CARGO_PROFILE_RELEASE_LTO", "false")
            .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "16")
            .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "2");
    }
    let Ok(output) = build.stderr(Stdio::inherit()).output() else {
        eprintln!("xtask codegen: cannot run cargo");
        return None;
    };
    if !output.status.success() {
        eprintln!("xtask codegen: the build failed");
        return None;
    }
    // Earlier builds leave their listings beside this one's; the artifact
    // cargo reports names the build that is current.
    let messages = String::from_utf8_lossy(&output.stdout);
    let Some(stem) = messages.rfind("libcrc_turbo-").and_then(|at| {
        let name = &messages[at + 3..];
        name.find(".rmeta").map(|end| name[..end].to_owned())
    }) else {
        eprintln!("xtask codegen: cargo did not report the library it built");
        return None;
    };
    let mut listing = String::new();
    for entry in fs::read_dir(&deps).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&stem) && name.ends_with(".s") {
            listing.push_str(&fs::read_to_string(entry.path()).ok()?);
            listing.push('\n');
        }
    }
    if listing.is_empty() {
        eprintln!(
            "xtask codegen: the build wrote no assembly under {}",
            deps.display()
        );
        return None;
    }
    let paths: Vec<String> = parse_snapshot(snap_text)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    Some(measure(&listing, &paths))
}

/// The mangled form of a path's components, in either mangling scheme: each is
/// its length and then its text, written one after another.
fn needle(path: &str) -> String {
    path.split("::")
        .map(|part| format!("{}{part}", part.len()))
        .collect()
}

/// Whether an assembly label is the function at `needle`, and not an item
/// nested in it or a function whose name it merely starts.
fn label_is(label: &str, needle: &str) -> bool {
    let Some(at) = label.find(needle) else {
        return false;
    };
    let rest = &label[at + needle.len()..];
    // v0 mangling ends at the name; the legacy scheme appends a hash.
    rest.is_empty()
        || (rest.len() == 20
            && rest.starts_with("17h")
            && rest.ends_with('E')
            && rest[3..19].bytes().all(|b| b.is_ascii_hexdigit()))
}

fn measure(listing: &str, paths: &[String]) -> BTreeMap<String, Vec<Shape>> {
    let needles: Vec<(String, String)> = paths
        .iter()
        .map(|path| (path.clone(), needle(path)))
        .collect();
    let mut found: BTreeMap<String, Vec<Shape>> = BTreeMap::new();
    let mut current: Option<(String, Shape)> = None;
    let mut in_asm = false;
    for raw in listing.lines() {
        let line = raw.trim();
        if current.is_none() {
            if let Some(label) = line.strip_suffix(':')
                && (label.contains("_ZN") || label.contains("_R"))
                && let Some((path, _)) = needles.iter().find(|(_, n)| label_is(label, n))
            {
                current = Some((
                    path.clone(),
                    Shape {
                        asm_hash: FNV_OFFSET,
                        ..Shape::default()
                    },
                ));
                in_asm = false;
            }
            continue;
        }
        let (_, shape) = current.as_mut().expect("checked above");
        if line.starts_with(".cfi_endproc") || line.starts_with(".size") {
            let (path, mut shape) = current.take().expect("checked above");
            if shape.asm == 0 {
                shape.asm_hash = 0;
            }
            found.entry(path).or_default().push(shape);
            continue;
        }
        if let Some(marker) = asm_marker(line) {
            in_asm = marker;
            continue;
        }
        let Some(mnemonic) = mnemonic(line) else {
            continue;
        };
        if in_asm {
            shape.asm += 1;
            for byte in mnemonic.bytes().chain(*b"\n") {
                shape.asm_hash = (shape.asm_hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
            }
        } else {
            shape.glue += 1;
            if matches!(mnemonic, "call" | "callq" | "bl" | "blr") {
                shape.calls += 1;
            }
        }
    }
    found
}

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// `Some(true)` where inline assembly starts, `Some(false)` where it ends.
fn asm_marker(line: &str) -> Option<bool> {
    let text = line.trim_start_matches(['#', '/', ';', ' ', '\t']);
    if text.starts_with("NO_APP") || text.starts_with("InlineAsm End") {
        Some(false)
    } else if text.starts_with("APP") || text.starts_with("InlineAsm Start") {
        Some(true)
    } else {
        None
    }
}

/// The instruction's mnemonic, or `None` for a label, a directive, a comment
/// or an empty line.
fn mnemonic(line: &str) -> Option<&str> {
    let word = line.split_whitespace().next()?;
    let first = word.as_bytes()[0];
    (first.is_ascii_alphabetic() && !word.ends_with(':')).then_some(word)
}

fn check(snap_text: &str, found: &BTreeMap<String, Vec<Shape>>, exact: bool) -> ExitCode {
    let mut failures = Vec::new();
    for (path, want) in parse_snapshot(snap_text) {
        let Some(shapes) = found.get(&path) else {
            // Off the pinned build a function with no assembly of its own may
            // have been inlined away; one that carries assembly must exist.
            if exact || want.asm > 0 {
                failures.push(format!("{path}: not in the build"));
            }
            continue;
        };
        // Split into codegen units, an inlinable function is built once per
        // unit that uses it, and every copy has to hold.
        if exact && shapes.len() != 1 {
            failures.push(format!(
                "{path}: {} functions match, expected one",
                shapes.len()
            ));
            continue;
        }
        for got in shapes {
            println!("{}", got.line(&path));
            if (got.asm_hash, got.asm) != (want.asm_hash, want.asm) {
                failures.push(format!(
                    "{path}: the inline assembly changed: {:016x}/{} in the snapshot, {:016x}/{} built",
                    want.asm_hash, want.asm, got.asm_hash, got.asm
                ));
            }
            if exact {
                if (got.glue, got.calls) != (want.glue, want.calls) {
                    failures.push(format!(
                        "{path}: glue={} calls={} in the snapshot, glue={} calls={} built",
                        want.glue, want.calls, got.glue, got.calls
                    ));
                }
            } else if got.glue > glue_limit(want.glue) || got.calls > want.calls {
                failures.push(format!(
                    "{path}: this compiler or profile grew it: glue={} calls={} pinned, glue={} calls={} built",
                    want.glue, want.calls, got.glue, got.calls
                ));
            }
        }
    }
    if failures.is_empty() {
        return ExitCode::SUCCESS;
    }
    for failure in &failures {
        eprintln!("xtask codegen: {failure}");
    }
    if exact {
        eprintln!("xtask codegen: if the change is meant, run `cargo xtask codegen bless`");
    }
    ExitCode::FAILURE
}

fn bless(snap_path: &Path, snap_text: &str, found: &BTreeMap<String, Vec<Shape>>) -> ExitCode {
    let mut out = String::new();
    let mut ok = true;
    for line in snap_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push_str(line);
        } else {
            let path = trimmed.split_whitespace().next().unwrap_or_default();
            match found.get(path).map(Vec::as_slice) {
                Some([shape]) => out.push_str(&shape.line(path)),
                Some(shapes) => {
                    eprintln!(
                        "xtask codegen: {path}: {} functions match, expected one",
                        shapes.len()
                    );
                    out.push_str(line);
                    ok = false;
                }
                None => {
                    eprintln!("xtask codegen: {path}: not in the build");
                    out.push_str(line);
                    ok = false;
                }
            }
        }
        out.push('\n');
    }
    if fs::write(snap_path, out).is_err() {
        eprintln!("xtask codegen: cannot write {}", snap_path.display());
        return ExitCode::FAILURE;
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\
\t.type\t_ZN9crc_turbo4arch4fold17h0123456789abcdefE,@function
_ZN9crc_turbo4arch4fold17h0123456789abcdefE:
\t.cfi_startproc
\tpushq\t%rbx
\tcallq\tmemcpy@PLT
\t#APP
\tcrc32q\t(%rdi), %rax
2:
\taddq\t$8, %rdi
\t#NO_APP
\tretq
\t.cfi_endproc
_ZN9crc_turbo4arch9fold_wide17h0123456789abcdefE:
\tretq
\t.cfi_endproc
_RNvNtCs1234_9crc_turbo4arch4fold:
\tret
\t.cfi_endproc
";

    #[test]
    fn counts_assembly_apart_from_what_the_compiler_wrote() {
        let found = measure(LISTING, &["crc_turbo::arch::fold".to_owned()]);
        let shapes = &found["crc_turbo::arch::fold"];
        // The legacy and the v0 label both name it; `fold_wide` does not.
        assert_eq!(shapes.len(), 2);
        assert_eq!((shapes[0].asm, shapes[0].glue, shapes[0].calls), (2, 3, 1));
        assert_ne!(shapes[0].asm_hash, 0);
        assert_eq!(
            (shapes[1].asm, shapes[1].asm_hash, shapes[1].glue),
            (0, 0, 1)
        );
    }

    #[test]
    fn the_assembly_hash_follows_mnemonics_and_not_operands() {
        let other = LISTING.replace("(%rdi), %rax", "(%rsi), %rcx");
        let swapped = LISTING.replace("addq\t$8", "subq\t$8");
        let paths = ["crc_turbo::arch::fold".to_owned()];
        let hash = |text: &str| measure(text, &paths)["crc_turbo::arch::fold"][0].asm_hash;
        assert_eq!(hash(LISTING), hash(&other));
        assert_ne!(hash(LISTING), hash(&swapped));
    }

    #[test]
    fn a_snapshot_line_round_trips() {
        let shape = Shape {
            asm_hash: 0xABCD,
            asm: 7,
            glue: 12,
            calls: 1,
        };
        let parsed = parse_snapshot(&format!("# note\n\n{}\n", shape.line("a::b")));
        assert_eq!(parsed, vec![("a::b".to_owned(), shape)]);
    }

    #[test]
    fn darwin_and_aarch64_markers_are_seen() {
        assert_eq!(asm_marker("## InlineAsm Start"), Some(true));
        assert_eq!(asm_marker("; InlineAsm End"), Some(false));
        assert_eq!(asm_marker("//APP"), Some(true));
        assert_eq!(asm_marker("//NO_APP"), Some(false));
        assert_eq!(asm_marker("addq $8, %rdi"), None);
    }
}
