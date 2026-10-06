//! Debugger snapshot harness for `debuggable`.
//!
//! Builds the fixture programs in `tests/fixtures` (pinned toolchain), runs each one under
//! every available debugger in batch mode, stops in `debuggable_fixture_stop(f: &Fixture)`,
//! prints `f`'s fields, normalizes the output and hands it to `insta`.
//!
//! Environment:
//! - `DEBUGGABLE_DEBUGGERS=gdb,lldb-18,lldb-20`: use exactly these (missing ones are an error).
//!   Unset: auto-detect `gdb`, `lldb`, `lldb-18` .. `lldb-22`, skipping what isn't installed.
//! - `DEBUGGABLE_PROFILES=dev,release-fat`: limit profiles (default: all four).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Cargo profiles every fixture is built and tested in.
pub const PROFILES: &[&str] = &["dev", "release", "release-thin", "release-fat"];

/// Which debugger family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// GNU GDB, driven through rustc's `rust-gdb` wrapper.
    Gdb,
    /// LLDB, with rustc's formatters and our loader imported explicitly.
    Lldb,
}

/// One installed debugger.
#[derive(Clone, Debug)]
pub struct Debugger {
    /// Family.
    pub kind: Kind,
    /// Executable name or path.
    pub bin: String,
    /// Snapshot id: `gdb15`, `lldb18`, `lldb22`, `applelldb1600`, ...
    pub id: String,
}

/// Repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// `tests/fixtures`.
pub fn fixtures_dir() -> PathBuf {
    repo_root().join("tests/fixtures")
}

fn profiles_from_env() -> Vec<String> {
    match std::env::var("DEBUGGABLE_PROFILES") {
        Ok(v) => v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        Err(_) => PROFILES.iter().map(|s| s.to_string()).collect(),
    }
}

/// Profiles selected for this run.
pub fn profiles() -> Vec<String> {
    profiles_from_env()
}

fn probe(bin: &str) -> Option<Debugger> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let first = text.lines().next()?.trim().to_string();
    let first_number = |s: &str| -> Option<u32> {
        s.split(|c: char| !c.is_ascii_digit()).find(|t| !t.is_empty())?.parse().ok()
    };
    if first.starts_with("GNU gdb") {
        // "GNU gdb (Ubuntu 15.1-1ubuntu1~24.04.1) 15.1": the version is the last token.
        let major = first_number(first.rsplit(' ').next()?)?;
        Some(Debugger { kind: Kind::Gdb, bin: bin.into(), id: format!("gdb{major}") })
    } else if let Some(rest) = first.strip_prefix("lldb version ") {
        let major = first_number(rest)?;
        Some(Debugger { kind: Kind::Lldb, bin: bin.into(), id: format!("lldb{major}") })
    } else if let Some(rest) = first.strip_prefix("lldb-") {
        // Apple: "lldb-1600.0.39.3"
        let major = first_number(rest)?;
        Some(Debugger { kind: Kind::Lldb, bin: bin.into(), id: format!("applelldb{major}") })
    } else {
        None
    }
}

/// Debuggers to test against (see module docs).
pub fn debuggers() -> Vec<Debugger> {
    if let Ok(list) = std::env::var("DEBUGGABLE_DEBUGGERS") {
        return list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|b| probe(b).unwrap_or_else(|| panic!("DEBUGGABLE_DEBUGGERS: `{b}` not found or not recognized")))
            .collect();
    }
    let mut seen = HashSet::new();
    ["gdb", "lldb", "lldb-18", "lldb-19", "lldb-20", "lldb-21", "lldb-22"]
        .iter()
        .filter_map(|b| probe(b))
        .filter(|d| seen.insert(d.id.clone()))
        .collect()
}

static BUILT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A command that runs in `tests/fixtures` with the pinned toolchain.
///
/// rustup sets `RUSTUP_TOOLCHAIN` for everything `cargo test` spawns, which would override
/// `tests/fixtures/rust-toolchain.toml`, so remove it. `DEBUGGABLE_FIXTURE_TOOLCHAIN` can
/// override the pin explicitly (e.g. where the pinned version can't be installed).
pub fn fixture_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(program);
    c.current_dir(fixtures_dir()).env_remove("RUSTUP_TOOLCHAIN");
    if let Ok(t) = std::env::var("DEBUGGABLE_FIXTURE_TOOLCHAIN") {
        c.env("RUSTUP_TOOLCHAIN", t);
    }
    c
}

fn cargo_in_fixtures() -> Command {
    let mut c = fixture_command("cargo");
    c.env_remove("RUSTFLAGS").env_remove("CARGO_TARGET_DIR");
    c
}

/// Build every fixture in `profile` (once per test process).
pub fn build(profile: &str) {
    let mut built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    if built.iter().any(|p| p == profile) {
        return;
    }
    let out = cargo_in_fixtures()
        .args(["build", "--workspace", "--profile", profile])
        .output()
        .expect("run cargo");
    assert!(out.status.success(), "fixture build failed ({profile}):\n{}", String::from_utf8_lossy(&out.stderr));
    built.push(profile.to_string());
}

/// Path of a fixture binary for a profile.
pub fn binary(fixture: &str, profile: &str) -> PathBuf {
    let dir = if profile == "dev" { "debug" } else { profile };
    fixtures_dir().join("target").join(dir).join(format!("fx-{fixture}"))
}

fn rustc_etc_dir() -> PathBuf {
    let out = fixture_command("rustc").args(["--print", "sysroot"]).output().unwrap();
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim()).join("lib/rustlib/etc")
}

/// Raw debugger output.
pub struct Run {
    /// stdout
    pub stdout: String,
    /// stderr
    pub stderr: String,
}

const MARK: &str = "@@@ ";

/// Run `bin` under `dbg`, stop in `debuggable_fixture_stop`, print each `f.<field>`.
/// Uses a UTF-8 locale so output doesn't depend on the machine's settings.
pub fn run(dbg: &Debugger, bin: &Path, fields: &[&str]) -> Run {
    run_with_locale(dbg, bin, fields, "C.UTF-8")
}

/// Like [`run`], with an explicit `LC_ALL` (e.g. `"C"` to test ASCII-only terminals).
pub fn run_with_locale(dbg: &Debugger, bin: &Path, fields: &[&str], locale: &str) -> Run {
    let target_dir = fixtures_dir().join("target");
    let mut cmd;
    match dbg.kind {
        Kind::Gdb => {
            cmd = fixture_command("rust-gdb"); // the pinned toolchain's wrapper and printers
            cmd.env("RUST_GDB", &dbg.bin);
            for a in [
                "set debuginfod enabled off".to_string(),
                format!("add-auto-load-safe-path {}", target_dir.display()),
                "set print pretty on".into(),
                "set width 0".into(),
                "set height 0".into(),
            ] {
                cmd.arg("-iex").arg(a);
            }
            cmd.args(["-nx", "-batch", "-ex", "break debuggable_fixture_stop", "-ex", "run"]);
            for f in fields {
                cmd.arg("-ex").arg(format!("echo {MARK}{f}\\n"));
                cmd.arg("-ex").arg(format!("print (*f).{f}"));
            }
        }
        Kind::Lldb => {
            // Same order as CodeLLDB and rust-lldb: rustc's formatters, then our loader.
            let etc = rustc_etc_dir();
            let loader = repo_root().join("cargo-debuggable/assets/debuggable_lldb.py");
            cmd = fixture_command(&dbg.bin);
            cmd.args(["-b", "-x"]);
            let mut pre = vec![format!("command script import {}", etc.join("lldb_lookup.py").display())];
            // Like rust-lldb: older toolchains register their formatters through
            // `lldb_commands`; newer ones ship without that file.
            if etc.join("lldb_commands").exists() {
                pre.push(format!("command source -s 1 {}", etc.join("lldb_commands").display()));
            }
            pre.push(format!("command script import {}", loader.display()));
            for a in pre {
                cmd.arg("-O").arg(a);
            }
            cmd.args(["-o", "breakpoint set -n debuggable_fixture_stop", "-o", "run"]);
            for f in fields {
                cmd.arg("-o").arg(format!("script print('{MARK}{f}')"));
                cmd.arg("-o").arg(format!("frame variable f->{f}"));
            }
        }
    }
    cmd.arg(bin).env("LC_ALL", locale).env("LANG", locale);
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {}: {e}", dbg.bin));
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn is_hex_addr(tok: &str) -> bool {
    let h = tok.strip_prefix("0x").unwrap_or("");
    // Real addresses are 12-16 hex digits; shorter values (e.g. LLDB's `U+0x00000071`
    // for a char, or a hex-formatted u32) are data and must stay in the snapshot.
    h.len() >= 9 && h.chars().all(|c| c.is_ascii_hexdigit())
}

/// Replace absolute paths and heap/stack addresses so snapshots are stable.
pub fn normalize(s: &str) -> String {
    let fixtures = fixtures_dir().display().to_string();
    let s = s.replace(&fixtures, "<fixtures>");
    let mut out = String::new();
    for line in s.lines() {
        let mut l = String::new();
        let mut word = String::new();
        for c in line.chars().chain(std::iter::once('\0')) {
            if c.is_ascii_alphanumeric() {
                word.push(c);
            } else {
                l.push_str(if is_hex_addr(&word) { "0x<addr>" } else { &word });
                word.clear();
                if c != '\0' {
                    l.push(c);
                }
            }
        }
        out.push_str(l.trim_end());
        out.push('\n');
    }
    out
}

/// Turn a run into snapshot text: one `## field` section per printed field.
/// Errors (breakpoint not hit, a field that printed nothing, any error from our
/// runtime or loader) are returned as `Err` so the test fails with the raw output.
pub fn sections(run: &Run, fields: &[&str]) -> Result<String, String> {
    let raw = format!("--- stdout\n{}\n--- stderr\n{}", run.stdout, run.stderr);
    let hit = run.stdout.contains("debuggable_fixture_stop")
        && (run.stdout.contains("Breakpoint 1,") || run.stdout.contains("stop reason = breakpoint"));
    if !hit {
        return Err(format!("breakpoint in debuggable_fixture_stop was not hit\n{raw}"));
    }
    // "Python Exception": GDB's report of an exception raised while printing (e.g. our
    // summary text not encodable in the host charset). GDB's std printers never raise on
    // the fixtures, so any occurrence is ours to investigate.
    for needle in ["<debuggable:", "<debuggable>", "debuggable_lldb.py\", line", "debuggable_runtime", "Python Exception"] {
        if run.stdout.contains(needle) || run.stderr.contains(needle) {
            return Err(format!("error from debuggable's own code (`{needle}`)\n{raw}"));
        }
    }
    let mut text = String::new();
    let mut lines = run.stdout.lines().peekable();
    for f in fields {
        let marker = format!("{MARK}{f}");
        while let Some(l) = lines.next() {
            if l == marker {
                break;
            }
        }
        let mut body = Vec::new();
        while let Some(l) = lines.peek() {
            // A section ends at the next marker (LLDB echoes `script print(...)` first) or
            // when the debugger tears the process down.
            if l.starts_with(MARK) || l.starts_with("(lldb) script print(") || l.starts_with("Process ") {
                break;
            }
            let l = lines.next().unwrap();
            if !l.starts_with("(lldb) ") {
                body.push(l); // drop LLDB's echo of the `frame variable` command
            }
        }
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        if body.is_empty() {
            return Err(format!("field `{f}` printed nothing\n{raw}"));
        }
        text.push_str(&format!("## {f}\n{}\n", body.join("\n")));
    }
    Ok(normalize(&text))
}
