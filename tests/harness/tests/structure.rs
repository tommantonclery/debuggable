//! Structural checks on the built binaries, independent of any debugger.

use debuggable_harness::{binary, build, fixtures_dir, profiles, repo_root};
use std::process::Command;

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// Entry names each fixture binary must carry (schema-v1 §3.1), including entries from
/// dependency crates and from a pure-data type with no functions.
const EXPECTED: &[(&str, &[&str])] = &[
    ("structs", &["fx_structs::geo::Point@0.1.0", "fx_structs::Meters@", "fx_structs::Account@", "fx_structs::Trip@"]),
    ("enums", &["fx_enums::Token@", "fx_enums::Glyph@", "fx_enums::Tagged@"]),
    ("items", &["fx_items::Slot@", "fx_items::SlotMap@", "fx_items::Stack@", "fx_items::RawBuf@"]),
    ("deps", &["lib2021::Celsius@0.3.1", "lib2021::Pair@0.3.1", "lib2024::Flag@0.3.1"]),
];

#[cfg(all(unix, not(target_vendor = "apple")))]
const RUNTIME: &str = "debuggable-runtime-gdb-v1.";

#[test]
fn every_entry_survives_linking_in_every_profile() {
    let mut missing = Vec::new();
    for profile in profiles() {
        build(&profile);
        for (fixture, names) in EXPECTED {
            let bytes = std::fs::read(binary(fixture, &profile)).unwrap();
            for n in *names {
                if !contains(&bytes, &format!("debuggable-v1-{n}")) {
                    missing.push(format!("{profile}/{fixture}: {n}"));
                }
            }
            #[cfg(all(unix, not(target_vendor = "apple")))]
            if !contains(&bytes, RUNTIME) {
                missing.push(format!("{profile}/{fixture}: GDB runtime entry"));
            }
        }
    }
    assert!(missing.is_empty(), "missing entries:\n{}", missing.join("\n"));
}

#[test]
fn entries_survive_strip() {
    if Command::new("strip").arg("--version").output().is_err() {
        eprintln!("SKIPPED: `strip` not found");
        return;
    }
    build("dev");
    let out = std::env::temp_dir().join(format!("debuggable-stripped-{}", std::process::id()));
    let st = Command::new("strip").arg("-o").arg(&out).arg(binary("structs", "dev")).status().unwrap();
    assert!(st.success());
    let bytes = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(contains(&bytes, "debuggable-v1-fx_structs::geo::Point@"), "entry removed by strip");
}

#[test]
fn disable_flag_emits_nothing() {
    let target = fixtures_dir().join("target/disabled");
    let out = Command::new("cargo")
        .current_dir(fixtures_dir())
        .env("RUSTFLAGS", "--cfg debuggable_disable")
        .arg("build")
        .arg("--workspace")
        .arg("--target-dir")
        .arg(&target)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("warning"), "the opt-out flag must not cause warnings:\n{stderr}");
    for (fixture, _) in EXPECTED {
        let bytes = std::fs::read(target.join("debug").join(format!("fx-{fixture}"))).unwrap();
        assert!(!contains(&bytes, "debuggable-v1-"), "{fixture}: descriptor entry present");
        assert!(!contains(&bytes, "debuggable-runtime-gdb"), "{fixture}: runtime entry present");
    }
}

#[test]
fn embedded_gdb_runtime_is_fresh() {
    let st = Command::new("python3")
        .current_dir(repo_root())
        .args(["tools/gen-runtime.py", "--check"])
        .status();
    match st {
        Ok(st) => assert!(st.success(), "run tools/gen-runtime.py"),
        Err(_) => eprintln!("SKIPPED: python3 not found"),
    }
}
