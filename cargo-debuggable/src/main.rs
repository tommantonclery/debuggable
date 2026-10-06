//! `cargo debuggable`: set up GDB, LLDB and VS Code to show `debuggable` visualizers, and
//! diagnose why they don't.

mod doctor;
mod edit;
mod locations;
mod setup;

use std::path::PathBuf;

const HELP: &str = "\
cargo-debuggable: debugger setup for the `debuggable` crate

USAGE:
    cargo debuggable setup [--gdb] [--lldb] [--vscode] [--dry-run]
    cargo debuggable setup --remove [--dry-run]
    cargo debuggable doctor [BINARY]

setup
    Configures every debugger it finds (or only those named):
      --gdb      trust this Cargo project's target directory in GDB (run once per project)
      --lldb     install the LLDB loader and import it from ~/.lldbinit
      --vscode   make CodeLLDB import the loader (VS Code settings, all projects)
      --dry-run  show what would change; write nothing
      --remove   undo everything setup has done (backups are kept)
    Only edits marked `debuggable` blocks; backs up each file before its first change.

doctor
    Checks debuggers, configuration and, given a BINARY, whether its visualizers will load.
";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("debuggable") {
        args.remove(0); // invoked as `cargo debuggable ...`
    }
    let result = match args.first().map(String::as_str) {
        Some("setup") => parse_setup(&args[1..]).and_then(setup::run).map(|()| 0),
        Some("doctor") => parse_doctor(&args[1..]).and_then(doctor::run),
        Some("-h" | "--help" | "help") | None => {
            print!("{HELP}");
            Ok(0)
        }
        Some("-V" | "--version") => {
            println!("cargo-debuggable {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{HELP}")),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}

fn parse_setup(args: &[String]) -> Result<setup::Options, String> {
    let mut o = setup::Options::default();
    for a in args {
        match a.as_str() {
            "--gdb" => o.gdb = true,
            "--lldb" => o.lldb = true,
            "--vscode" => o.vscode = true,
            "--dry-run" => o.dry_run = true,
            "--remove" => o.remove = true,
            "-h" | "--help" => return Err(HELP.into()),
            other => return Err(format!("unknown option `{other}` for setup\n\n{HELP}")),
        }
    }
    Ok(o)
}

fn parse_doctor(args: &[String]) -> Result<Option<PathBuf>, String> {
    match args {
        [] => Ok(None),
        [one] if !one.starts_with('-') => Ok(Some(PathBuf::from(one))),
        _ => Err(format!("usage: cargo debuggable doctor [BINARY]\n\n{HELP}")),
    }
}
