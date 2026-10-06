//! Where things live. Everything derives from `HOME` and the XDG variables, so tests can
//! point the tool at a temporary home directory.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| "HOME is not set".to_string())
}

fn xdg(var: &str, default: &str) -> Result<PathBuf, String> {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => Ok(home()?.join(default)),
    }
}

/// Where the LLDB loader is installed.
pub fn loader_path() -> Result<PathBuf, String> {
    let base = if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support")
    } else {
        xdg("XDG_DATA_HOME", ".local/share")?
    };
    Ok(base.join("debuggable/lldb/debuggable_lldb.py"))
}

/// The GDB init file to edit: `~/.gdbinit` if it exists, otherwise the XDG location that
/// GDB 11+ reads (older GDB only reads `~/.gdbinit`).
pub fn gdbinit(gdb_major: Option<u32>) -> Result<PathBuf, String> {
    let classic = home()?.join(".gdbinit");
    if classic.exists() || gdb_major.is_some_and(|m| m < 11) {
        return Ok(classic);
    }
    Ok(xdg("XDG_CONFIG_HOME", ".config")?.join("gdb/gdbinit"))
}

/// Both GDB init files, for reading (doctor).
pub fn gdbinit_candidates() -> Result<Vec<PathBuf>, String> {
    Ok(vec![home()?.join(".gdbinit"), xdg("XDG_CONFIG_HOME", ".config")?.join("gdb/gdbinit")])
}

pub fn lldbinit() -> Result<PathBuf, String> {
    Ok(home()?.join(".lldbinit"))
}

/// VS Code settings files whose directory exists: the remote-server machine settings
/// (WSL, SSH, containers) and the local user settings of VS Code and VSCodium.
pub fn vscode_settings() -> Result<Vec<PathBuf>, String> {
    let home = home()?;
    let mut candidates = vec![home.join(".vscode-server/data/Machine")];
    if cfg!(target_os = "macos") {
        let app = home.join("Library/Application Support");
        candidates.push(app.join("Code/User"));
        candidates.push(app.join("VSCodium/User"));
    } else {
        let cfg = xdg("XDG_CONFIG_HOME", ".config")?;
        candidates.push(cfg.join("Code/User"));
        candidates.push(cfg.join("VSCodium/User"));
    }
    Ok(candidates.into_iter().filter(|d| d.is_dir()).map(|d| d.join("settings.json")).collect())
}

/// `~/x/y` instead of `/home/me/x/y`, for messages.
pub fn pretty(p: &Path) -> String {
    match home() {
        Ok(h) => match p.strip_prefix(&h) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        },
        Err(_) => p.display().to_string(),
    }
}

/// The current Cargo project's target directory, via `cargo metadata`.
pub fn cargo_target_dir() -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !out.status.success() {
        return Err("not inside a Cargo project (run this in your project's directory)".into());
    }
    let json = String::from_utf8_lossy(&out.stdout);
    json_string_field(&json, "target_directory")
        .map(PathBuf::from)
        .ok_or_else(|| "unexpected `cargo metadata` output".to_string())
}

/// The value of `"field":"..."` in a JSON document, unescaped (enough for cargo metadata).
fn json_string_field(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\":");
    let start = json.find(&key)? + key.len();
    let rest = json[start..].trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                c => out.push(c), // \" \\ \/
            },
            c => out.push(c),
        }
    }
    None
}

/// Major version and display string of a debugger, if it runs.
pub fn debugger_version(bin: &str) -> Option<(u32, String)> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    let first = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    let version = if first.starts_with("GNU gdb") {
        first.rsplit(' ').next()?.to_string()
    } else if let Some(v) = first.strip_prefix("lldb version ") {
        v.split_whitespace().next()?.to_string()
    } else {
        format!("Apple {}", first.strip_prefix("lldb-")?)
    };
    let major = version.trim_start_matches("Apple ").split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    Some((major, version))
}

/// Installed LLDB executables: `lldb` and versioned names like `lldb-18`.
pub fn lldbs() -> Vec<(String, u32, String)> {
    let mut out = Vec::new();
    for bin in std::iter::once("lldb".to_string()).chain((14..=24).rev().map(|v| format!("lldb-{v}"))) {
        if let Some((major, v)) = debugger_version(&bin) {
            if !out.iter().any(|(_, m, _): &(String, u32, String)| *m == major) {
                out.push((bin, major, v));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cargo_metadata_fields() {
        let json = r#"{"packages":[],"target_directory":"/home/u/my \"proj\"/target","version":1}"#;
        assert_eq!(json_string_field(json, "target_directory").unwrap(), "/home/u/my \"proj\"/target");
        assert_eq!(json_string_field(r#"{"target_directory": "C:\\x\\target"}"#, "target_directory").unwrap(), "C:\\x\\target");
        assert_eq!(json_string_field("{}", "target_directory"), None);
    }
}
