//! `cargo debuggable doctor [binary]`: explain why visualizers do or don't show up.

use crate::edit::{self, Comment};
use crate::locations::{self, pretty};
use crate::setup::LOADER;
use object::{Object, ObjectSection};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default)]
struct Report {
    failures: usize,
    warnings: usize,
}

impl Report {
    fn ok(&self, msg: impl AsRef<str>) {
        println!("  ok    {}", msg.as_ref());
    }
    fn info(&self, msg: impl AsRef<str>) {
        println!("  info  {}", msg.as_ref());
    }
    fn warn(&mut self, msg: impl AsRef<str>) {
        self.warnings += 1;
        println!("  warn  {}", msg.as_ref());
    }
    fn fail(&mut self, msg: impl AsRef<str>) {
        self.failures += 1;
        println!("  FAIL  {}", msg.as_ref());
    }
    /// Continuation line for the previous message.
    fn more(&self, msg: impl AsRef<str>) {
        println!("        {}", msg.as_ref());
    }
}

/// Exit status: non-zero if anything failed.
pub fn run(binary: Option<PathBuf>) -> Result<i32, String> {
    let mut r = Report::default();
    println!("cargo-debuggable {} doctor\n", env!("CARGO_PKG_VERSION"));

    println!("Debuggers");
    let gdb = locations::debugger_version("gdb");
    match &gdb {
        Some((major, v)) => {
            r.ok(format!("gdb {v}"));
            if *major < 11 && !locations::home()?.join(".gdbinit").exists() {
                r.warn("GDB < 11 only reads ~/.gdbinit; `setup` writes there for this version");
            }
        }
        None => r.info("gdb not found"),
    }
    let lldbs = locations::lldbs();
    for (bin, major, v) in &lldbs {
        r.ok(format!("{bin} {v}"));
        if *major == 18 {
            r.info("LLDB 18: rustc's own formatters crash on std enums such as Option (upstream issue);");
            r.more("debuggable types are unaffected. CodeLLDB bundles a newer LLDB.");
        }
    }
    if lldbs.is_empty() {
        r.info("lldb not found on PATH (CodeLLDB bundles its own)");
    }
    if gdb.is_none() && lldbs.is_empty() && locations::vscode_settings()?.is_empty() {
        r.fail("no debugger found: install gdb or lldb, or VS Code with CodeLLDB");
    }

    println!("\nConfiguration");
    let gdb_trusted = check_gdbinit(&mut r)?;
    check_lldb(&mut r)?;

    if let Some(bin) = binary {
        println!("\nBinary {}", pretty(&bin));
        check_binary(&mut r, &bin, gdb.is_some(), &gdb_trusted)?;
    } else {
        println!("\nTip: pass a binary to check it too: `cargo debuggable doctor target/debug/<name>`");
    }

    println!();
    match (r.failures, r.warnings) {
        (0, 0) => println!("All good."),
        (0, w) => println!("{w} warning(s)."),
        (f, w) => println!("{f} problem(s), {w} warning(s)."),
    }
    Ok(if r.failures > 0 { 1 } else { 0 })
}

/// Directories trusted by our gdbinit blocks (or anything that looks like a safe-path line).
fn check_gdbinit(r: &mut Report) -> Result<Vec<String>, String> {
    let mut trusted = Vec::new();
    for path in locations::gdbinit_candidates()? {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if let Some(lines) = edit::block_lines(&text, Comment::Hash) {
            let dirs: Vec<String> =
                lines.iter().filter_map(|l| l.strip_prefix("add-auto-load-safe-path ")).map(str::to_string).collect();
            r.ok(format!("{}: GDB trusts {} project director{}", pretty(&path), dirs.len(), if dirs.len() == 1 { "y" } else { "ies" }));
            trusted.extend(dirs);
        }
    }
    if trusted.is_empty() {
        r.info("GDB: no projects trusted yet (run `cargo debuggable setup` in a project)");
    }
    Ok(trusted)
}

fn check_lldb(r: &mut Report) -> Result<(), String> {
    let loader = locations::loader_path()?;
    match std::fs::read_to_string(&loader) {
        Ok(text) if text == LOADER => r.ok(format!("LLDB loader installed and current: {}", pretty(&loader))),
        Ok(_) => r.warn(format!("LLDB loader at {} is from another version: run `cargo debuggable setup`", pretty(&loader))),
        Err(_) => r.info("LLDB loader not installed (run `cargo debuggable setup` to use LLDB or VS Code)"),
    }
    let imports = |text: &str| text.contains("debuggable_lldb.py");
    let lldbinit = locations::lldbinit()?;
    if std::fs::read_to_string(&lldbinit).map(|t| imports(&t)).unwrap_or(false) {
        r.ok(format!("{}: LLDB imports the loader", pretty(&lldbinit)));
    } else {
        r.info(format!("{}: no loader import (command-line LLDB won't use debuggable)", pretty(&lldbinit)));
    }
    for settings in locations::vscode_settings()? {
        if std::fs::read_to_string(&settings).map(|t| imports(&t)).unwrap_or(false) {
            r.ok(format!("{}: CodeLLDB imports the loader", pretty(&settings)));
        } else {
            r.info(format!("{}: no loader import (CodeLLDB won't use debuggable)", pretty(&settings)));
        }
    }
    Ok(())
}

struct Entries {
    descriptors: usize,
    runtime: Option<String>,
    rustc_printers: bool,
}

fn entries(data: &[u8]) -> Entries {
    let mut e = Entries { descriptors: 0, runtime: None, rustc_printers: false };
    for entry in data.split(|b| *b == 0) {
        let name = entry.split(|b| *b == b'\n').next().unwrap_or_default();
        if name.starts_with(b"\x04debuggable-v1-") {
            e.descriptors += 1;
        } else if let Some(v) = name.strip_prefix(b"\x04debuggable-runtime-gdb-") {
            e.runtime = Some(String::from_utf8_lossy(v).into_owned());
        } else if name.ends_with(b"gdb_load_rust_pretty_printers.py") {
            e.rustc_printers = true;
        }
    }
    e
}

fn check_binary(r: &mut Report, bin: &Path, have_gdb: bool, trusted: &[String]) -> Result<(), String> {
    let data = std::fs::read(bin).map_err(|e| format!("reading {}: {e}", bin.display()))?;
    let file = match object::File::parse(&*data) {
        Ok(f) => f,
        Err(e) => {
            let fat = data.len() >= 4 && matches!(data[..4], [0xca, 0xfe, 0xba, 0xbe] | [0xbe, 0xba, 0xfe, 0xca]);
            if fat {
                r.fail("universal (fat) macOS binary: check one architecture, e.g. `lipo -thin arm64 -output x <binary>`");
            } else {
                r.fail(format!("not an executable this tool can read ({e})"));
            }
            return Ok(());
        }
    };
    let (section, is_elf) = match file.format() {
        object::BinaryFormat::Elf => (".debug_gdb_scripts", true),
        object::BinaryFormat::MachO => ("__debuggable", false),
        other => {
            r.fail(format!("{other:?} binaries are not supported in v0.1 (Linux ELF and macOS Mach-O are)"));
            return Ok(());
        }
    };
    let found = file.section_by_name(section).and_then(|s| s.data().ok()).map(entries);
    let has_debug_info = file.section_by_name(".debug_info").is_some() || file.has_debug_symbols();

    match &found {
        Some(e) if e.descriptors > 0 => {
            r.ok(format!("{} debuggable type(s) embedded", e.descriptors));
            if is_elf {
                match &e.runtime {
                    Some(v) => r.ok(format!("GDB runtime {v} embedded")),
                    None => r.fail("descriptors but no GDB runtime entry: please report this as a bug"),
                }
            }
        }
        _ => {
            r.fail("no debuggable entries in this binary. Likely causes:");
            r.more("- no crate in the build uses #[derive(Debuggable)]");
            r.more("- built with RUSTFLAGS=\"--cfg debuggable_disable\"");
            r.more("- a target without support in v0.1 (only Linux ELF and macOS are supported)");
        }
    }
    if !has_debug_info {
        if is_elf {
            r.fail("no debug info: debuggers can't show variables. Build with `debug = true` (the dev profile has it)");
        } else {
            r.info("no debug info in the executable itself; on macOS it lives in object files or a .dSYM");
        }
    }

    let has_entries = found.as_ref().is_some_and(|e| e.descriptors > 0);
    if is_elf && have_gdb && has_entries {
        let abs = bin.canonicalize().unwrap_or_else(|_| bin.to_path_buf());
        if trusted.iter().any(|dir| abs.starts_with(dir)) || safe_path_allows(&abs) {
            r.ok("GDB will auto-load the visualizers (binary is under a trusted directory)");
        } else {
            r.fail("GDB won't load the visualizers: the binary is outside GDB's auto-load safe-path");
            r.more("fix: run `cargo debuggable setup --gdb` in the binary's Cargo project");
        }
    }
    if is_elf && have_gdb && found.as_ref().is_some_and(|e| e.rustc_printers) {
        r.info("plain `gdb` prints \"Missing auto-load script ... gdb_load_rust_pretty_printers.py\".");
        r.more("That's rustc's std-type printers, which only `rust-gdb` can find. It's harmless;");
        r.more("use `rust-gdb` to see std types (String, Vec, ...) formatted too.");
    }
    Ok(())
}

/// Ask GDB itself (with the user's init files) whether a path is in its safe-path.
fn safe_path_allows(abs: &Path) -> bool {
    let Ok(out) = Command::new("gdb").args(["-batch", "-ex", "show auto-load safe-path"]).output() else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(list) = text.split(" is ").nth(1) else { return false };
    list.trim().trim_end_matches('.').trim_matches('"').split(':').any(|dir| {
        let dir = dir.trim();
        !dir.is_empty() && !dir.starts_with('$') && (dir == "/" || abs.starts_with(dir))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_entries() {
        let data = b"\x04debuggable-v1-a::A@1.0\nimport gdb\n\0\x04debuggable-v1-a::B@1.0\nx\n\0\x04debuggable-runtime-gdb-v1.2\nimport zlib\n\0\x01gdb_load_rust_pretty_printers.py\0";
        let e = entries(data);
        assert_eq!(e.descriptors, 2);
        assert_eq!(e.runtime.as_deref(), Some("v1.2"));
        assert!(e.rustc_printers);
    }
}
