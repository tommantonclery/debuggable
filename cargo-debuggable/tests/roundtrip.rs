//! `setup` then `setup --remove` against a temporary HOME: files are edited only inside
//! marked blocks, setup is idempotent, a hand-made configuration is left alone, and
//! removal restores every file byte-for-byte. Needs no debugger installed (explicit flags).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn tool(home: &Path, project: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-debuggable"))
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{args:?} failed:\n{text}\n{}", String::from_utf8_lossy(&out.stderr));
    text
}

fn write(path: PathBuf, text: &str) -> (PathBuf, String) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    (path, text.to_string())
}

#[test]
fn setup_is_idempotent_and_remove_restores_everything() {
    let root = std::env::temp_dir().join(format!("debuggable-roundtrip-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("proj");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n").unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();

    let originals = [
        write(home.join(".config/gdb/gdbinit"), "set history save on\n"),
        write(home.join(".lldbinit"), "settings set target.x86-disassembly-flavor intel\n"),
        write(home.join(".config/Code/User/settings.json"), "{\n    // mine\n    \"editor.fontSize\": 14\n}\n"),
    ];
    // configured by hand (as during development): must be left alone
    let manual = write(
        home.join(".vscode-server/data/Machine/settings.json"),
        "{\n    \"lldb.launch.preRunCommands\": [\n    \"command script import /x/debuggable_lldb.py\"\n    ]\n}\n",
    );

    let flags = ["setup", "--gdb", "--lldb", "--vscode"];
    let dry = tool(&home, &project, &[&flags[..], &["--dry-run"]].concat());
    assert!(dry.contains("would edit"), "{dry}");
    for (path, text) in originals.iter().chain([&manual]) {
        assert_eq!(&fs::read_to_string(path).unwrap(), text, "dry run must not write {}", path.display());
    }

    let first = tool(&home, &project, &flags);
    assert!(first.contains("set up by hand"), "{first}");
    let gdbinit = fs::read_to_string(&originals[0].0).unwrap();
    assert!(gdbinit.starts_with("set history save on\n# >>> debuggable"), "{gdbinit}");
    assert!(gdbinit.contains("add-auto-load-safe-path ") && gdbinit.contains("proj"), "{gdbinit}");
    assert!(fs::read_to_string(&originals[1].0).unwrap().contains("command script import "));
    assert!(fs::read_to_string(&originals[2].0).unwrap().contains("\"lldb.launch.preRunCommands\""));
    assert!(home.join(".local/share/debuggable/lldb/debuggable_lldb.py").exists());
    assert_eq!(fs::read_to_string(&manual.0).unwrap(), manual.1, "hand-made config untouched");

    let second = tool(&home, &project, &flags);
    assert!(second.contains("Nothing to change."), "not idempotent:\n{second}");

    tool(&home, &project, &["setup", "--remove"]);
    for (path, text) in originals.iter().chain([&manual]) {
        assert_eq!(&fs::read_to_string(path).unwrap(), text, "{} not restored exactly", path.display());
    }
    assert!(!home.join(".local/share/debuggable/lldb/debuggable_lldb.py").exists());
    let _ = fs::remove_dir_all(&root);
}
