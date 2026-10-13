//! Structural checks on the built binaries, independent of any debugger.

use debuggable_harness::{binary, build, fixture_command, fixtures_dir, profiles, repo_root};
use std::process::Command;

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// Every `debuggable` entry in a binary, as (name, whole entry in bytes, descriptor JSON).
/// Entries are `0x04 <name> 0x0A <body> 0x00` (schema-v1 §3), so they can be read straight out
/// of the file without an ELF parser, as `doctor` does from the section.
#[cfg(all(unix, not(target_vendor = "apple")))]
fn entries(bytes: &[u8]) -> Vec<(String, usize, Option<String>)> {
    const MARK: &[u8] = b"\x04debuggable-";
    let (mut out, mut at) = (Vec::new(), 0);
    while let Some(off) = bytes[at..].windows(MARK.len()).position(|w| w == MARK) {
        let start = at + off;
        let end = bytes[start..].iter().position(|b| *b == 0).map_or(bytes.len(), |n| start + n);
        let entry = &bytes[start..end];
        let name = String::from_utf8_lossy(&entry[1..entry.iter().position(|b| *b == b'\n').unwrap_or(entry.len())]);
        let json = entry
            .windows(4)
            .position(|w| w == b"r'''")
            .map(|i| &entry[i + 4..])
            .and_then(|rest| rest.windows(3).position(|w| w == b"'''").map(|i| &rest[..i]))
            .map(|j| String::from_utf8_lossy(j).into_owned());
        out.push((name.into_owned(), end - start + 1, json));
        at = end + 1;
    }
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// Entry names each fixture binary must carry (schema-v1 §3.1), including entries from
/// dependency crates and from a pure-data type with no functions.
const EXPECTED: &[(&str, &[&str])] = &[
    ("structs", &["fx_structs::geo::Point@0.1.0", "fx_structs::Meters@", "fx_structs::Account@", "fx_structs::Trip@"]),
    ("enums", &["fx_enums::Token@", "fx_enums::Glyph@", "fx_enums::Tagged@", "fx_enums::Msg@"]),
    ("items", &["fx_items::Slot@", "fx_items::SlotMap@", "fx_items::Stack@", "fx_items::RawBuf@"]),
    ("slots", &["fx_slots::Slab@", "fx_slots::Arena@", "fx_slots::SlotMap@"]),
    ("alternatives", &["fx_alternatives::SmallVec@", "fx_alternatives::TinyVec@", "fx_alternatives::Holder@"]),
    ("text", &["fx_text::InlineStr@", "fx_text::Tag@", "fx_text::Buf@", "fx_text::RawText@"]),
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

/// `cargo debuggable doctor` finds every fixture type in the debug info under the path its
/// descriptor names (schema-v1 §5), in every profile. ELF only: on macOS the debug info is not
/// in the executable.
#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn doctor_matches_every_type_in_debug_info() {
    let mut problems = Vec::new();
    for profile in profiles() {
        build(&profile);
        for (fixture, _) in EXPECTED {
            let bin = binary(fixture, &profile);
            let out = Command::new(env!("CARGO"))
                .current_dir(repo_root())
                .args(["run", "-q", "-p", "cargo-debuggable", "--", "debuggable", "doctor"])
                .arg(&bin)
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            // Only the binary's section: the configuration checks depend on this machine.
            let binary_part = text.split_once("\nBinary ").map_or("", |(_, b)| b);
            let all_found = binary_part.lines().any(|l| {
                l.trim_start()
                    .strip_prefix("ok    ")
                    .and_then(|m| m.split_once(" described type(s) found"))
                    .is_some_and(|(counts, _)| matches!(counts.split_once(" of "), Some((a, b)) if a == b))
            });
            if !all_found || binary_part.contains("  warn  ") {
                problems.push(format!("{profile}/{fixture}:\n{text}{}", String::from_utf8_lossy(&out.stderr)));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Entries sit in an allocated section, so `strip` keeps them; `--strip-debug` only drops
/// sections BFD marks as debugging, which excludes allocated ones. Both are claimed in
/// `docs/compatibility.md`, so both are checked.
#[test]
fn entries_survive_strip() {
    if Command::new("strip").arg("--version").output().is_err() {
        eprintln!("SKIPPED: `strip` not found");
        return;
    }
    build("dev");
    for (i, flags) in [&[][..], &["--strip-debug"][..]].iter().enumerate() {
        let out = std::env::temp_dir().join(format!("debuggable-stripped-{}-{i}", std::process::id()));
        let st = Command::new("strip").args(*flags).arg("-o").arg(&out).arg(binary("structs", "dev")).status().unwrap();
        assert!(st.success(), "`strip {}` failed", flags.join(" "));
        let bytes = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        assert!(contains(&bytes, "debuggable-v1-fx_structs::geo::Point@"), "entry removed by `strip {}`", flags.join(" "));
    }
}

/// The size budget in schema-v1 §8, on a fixture binary. ELF only: Mach-O carries no runtime
/// entry. The limits have headroom over what entries cost today (145 B of framing, 6,487 B of
/// runtime at minor 7); if one trips, remeasure, then move it deliberately and say so in §8.
#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn entries_stay_within_the_size_budget() {
    const FRAMING_MAX: usize = 160; // entry minus its name and its JSON
    const JSON_MAX: usize = 250;
    const RUNTIME_MAX: usize = 7 * 1024;
    build("dev");
    let bytes = std::fs::read(binary("structs", "dev")).unwrap();
    let found = entries(&bytes);
    let mut over = Vec::new();
    let mut descriptors = 0;
    let mut runtimes = 0;
    for (name, size, json) in &found {
        match json {
            Some(json) => {
                descriptors += 1;
                let framing = size - name.len() - json.len();
                if json.len() > JSON_MAX {
                    over.push(format!("{name}: JSON is {} B, over the {JSON_MAX} B budget", json.len()));
                }
                if framing > FRAMING_MAX {
                    over.push(format!("{name}: framing is {framing} B, over the {FRAMING_MAX} B budget"));
                }
            }
            None => {
                runtimes += 1;
                if *size > RUNTIME_MAX {
                    over.push(format!("{name}: {size} B, over the {RUNTIME_MAX} B budget"));
                }
            }
        }
    }
    assert!(descriptors >= 4 && runtimes == 1, "expected 4 descriptors and 1 runtime, found: {found:?}");
    assert!(
        over.is_empty(),
        "schema-v1 §8:\n{}",
        over.join(
            "
"
        )
    );
}

#[test]
fn disable_flag_emits_nothing() {
    let target = fixtures_dir().join("target/disabled");
    let out = fixture_command("cargo")
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
    let st = Command::new("python3").current_dir(repo_root()).args(["tools/gen-runtime.py", "--check"]).status();
    match st {
        Ok(st) => assert!(st.success(), "run tools/gen-runtime.py"),
        Err(_) => eprintln!("SKIPPED: python3 not found"),
    }
}
