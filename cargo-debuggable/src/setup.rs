//! `cargo debuggable setup`: configure GDB, LLDB and VS Code. Idempotent; `--remove` undoes
//! everything it ever did; `--dry-run` writes nothing.

use crate::edit::{self, Comment, JsonEdit};
use crate::locations::{self, pretty};
use std::path::{Path, PathBuf};

/// The LLDB loader, embedded at build time.
pub const LOADER: &str = include_str!("../assets/debuggable_lldb.py");

#[derive(Default)]
pub struct Options {
    pub gdb: bool,
    pub lldb: bool,
    pub vscode: bool,
    pub dry_run: bool,
    pub remove: bool,
}

struct Ctx {
    dry_run: bool,
    changed: usize,
}

impl Ctx {
    /// Write `new` to `path` if it differs; back up an existing file once, first.
    fn write(&mut self, path: &Path, new: &str, what: &str) -> Result<(), String> {
        let old = std::fs::read_to_string(path).unwrap_or_default();
        if old == new {
            println!("  unchanged  {}  ({what})", pretty(path));
            return Ok(());
        }
        self.changed += 1;
        let verb = match (self.dry_run, path.exists()) {
            (true, true) => "would edit",
            (true, false) => "would create",
            (false, true) => "edited    ",
            (false, false) => "created   ",
        };
        println!("  {verb} {}  ({what})", pretty(path));
        if self.dry_run {
            return Ok(());
        }
        if path.exists() {
            let backup = backup_path(path);
            if !backup.exists() {
                std::fs::copy(path, &backup).map_err(|e| format!("backing up {}: {e}", pretty(path)))?;
                println!("             backup: {}", pretty(&backup));
            }
        } else if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", pretty(dir)))?;
        }
        std::fs::write(path, new).map_err(|e| format!("writing {}: {e}", pretty(path)))
    }
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".debuggable.bak");
    path.with_file_name(name)
}

pub fn run(mut o: Options) -> Result<(), String> {
    let explicit = o.gdb || o.lldb || o.vscode;
    let gdb = locations::debugger_version("gdb");
    let lldbs = locations::lldbs();
    let vscode = locations::vscode_settings()?;
    if !explicit {
        o.gdb = o.remove || gdb.is_some();
        o.lldb = o.remove || !lldbs.is_empty();
        o.vscode = o.remove || !vscode.is_empty();
    }
    let mut cx = Ctx { dry_run: o.dry_run, changed: 0 };
    if o.dry_run {
        println!("Dry run: nothing will be written.\n");
    }

    if o.gdb {
        println!("GDB");
        gdb_setup(&mut cx, gdb.as_ref().map(|g| g.0), o.remove, explicit)?;
    }
    if o.lldb || o.vscode {
        let loader = locations::loader_path()?;
        if o.remove {
            if loader.exists() {
                cx.changed += 1;
                println!("  {} {}", if cx.dry_run { "would remove" } else { "removed   " }, pretty(&loader));
                if !cx.dry_run {
                    std::fs::remove_file(&loader).map_err(|e| format!("removing {}: {e}", pretty(&loader)))?;
                }
            }
        } else {
            println!("LLDB loader");
            cx.write(&loader, LOADER, "installed for LLDB and CodeLLDB")?;
        }
        let import = format!("command script import {}", loader.display());
        if o.lldb {
            println!("LLDB (command line, rust-lldb)");
            if lldbs.is_empty() && !o.remove {
                println!("  note       no `lldb` found on PATH; configuring anyway");
            }
            let path = locations::lldbinit()?;
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let base = edit::remove_block(&text, Comment::Hash);
            if !o.remove && base.contains("debuggable_lldb.py") {
                println!("  ok         {} already imports a debuggable loader (set up by hand); left as is", pretty(&path));
            } else if path.exists() || !o.remove {
                let lines = if o.remove { vec![] } else { vec![import.clone()] };
                cx.write(&path, &edit::set_hash_block(&text, &lines), "imports the loader")?;
            }
        }
        if o.vscode {
            println!("VS Code (CodeLLDB)");
            if vscode.is_empty() {
                println!("  skipped    no VS Code settings directory found");
            }
            for path in &vscode {
                let text = std::fs::read_to_string(path).unwrap_or_default();
                if o.remove && !path.exists() {
                    continue;
                }
                match edit::set_vscode_command(&text, if o.remove { None } else { Some(&import) }) {
                    Ok(JsonEdit::Write(new)) => cx.write(path, &new, "lldb.launch.preRunCommands")?,
                    Ok(JsonEdit::AlreadyManual) => println!(
                        "  ok         {} already imports a debuggable loader (set up by hand); left as is",
                        pretty(path)
                    ),
                    Err(e) => println!(
                        "  skipped    {}: {e}\n             add this to \"lldb.launch.preRunCommands\" yourself: \"{import}\"",
                        pretty(path)
                    ),
                }
            }
        }
    }

    println!();
    match (o.remove, cx.changed, o.dry_run) {
        (_, 0, _) => println!("Nothing to change."),
        (true, n, false) => println!("Removed debuggable's configuration ({n} change(s)). Backups (*.debuggable.bak) were kept."),
        (false, n, false) => println!("Done ({n} change(s)). Check with `cargo debuggable doctor <binary>`."),
        (_, n, true) => println!("{n} change(s) would be made. Run again without --dry-run to apply."),
    }
    Ok(())
}

fn gdb_setup(cx: &mut Ctx, gdb_major: Option<u32>, remove: bool, explicit: bool) -> Result<(), String> {
    if remove {
        // Both locations: setup may have written either over time.
        for path in locations::gdbinit_candidates()? {
            if let Ok(text) = std::fs::read_to_string(&path) {
                cx.write(&path, &edit::set_hash_block(&text, &[]), "trusted project directories")?;
            }
        }
        return Ok(());
    }
    let path = locations::gdbinit(gdb_major)?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines = edit::block_lines(&text, Comment::Hash).unwrap_or_default();
    let target = match locations::cargo_target_dir() {
        Ok(t) => t,
        Err(e) if !explicit => {
            println!("  skipped    {e}");
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let line = format!("add-auto-load-safe-path {}", target.display());
    if !lines.contains(&line) {
        lines.push(line);
    }
    let before = cx.changed;
    cx.write(&path, &edit::set_hash_block(&text, &lines), &format!("trusts {}", pretty(&target)))?;
    if cx.changed == before {
        return Ok(());
    }
    println!("  note       GDB runs scripts from binaries under trusted directories only; this trusts");
    println!("             this project's build output. Run setup in each project you debug.");
    println!("  tip        use `rust-gdb` instead of `gdb` to also see std types (String, Vec, ...) nicely");
    Ok(())
}
