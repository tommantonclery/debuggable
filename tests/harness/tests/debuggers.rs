//! Snapshot tests: every fixture x every available debugger x every profile.
//!
//! Review changes with `cargo insta test --review -p debuggable-harness`.
//! Snapshots are per debugger major version (`gdb15`, `lldb18`, `lldb22`, ...) because
//! debuggers and rustc's own formatters render std types differently across versions.

use debuggable_harness::{binary, build, debuggers, profiles, run, run_with_locale, sections, Kind};

fn check(fixture: &str, fields: &[&str]) {
    let dbgs = debuggers();
    if dbgs.is_empty() {
        eprintln!("SKIPPED {fixture}: no debugger found (install gdb or lldb, or set DEBUGGABLE_DEBUGGERS)");
        return;
    }
    let mut failures = Vec::new();
    for profile in profiles() {
        build(&profile);
        let bin = binary(fixture, &profile);
        for dbg in &dbgs {
            let name = format!("{fixture}__{}__{profile}", dbg.id);
            match sections(&run(dbg, &bin, fields), fields) {
                Ok(text) => {
                    insta::with_settings!({ snapshot_path => "snapshots", prepend_module_to_snapshot => false, omit_expression => true }, {
                        insta::assert_snapshot!(name, text);
                    });
                }
                Err(e) => failures.push(format!("==== {name}\n{e}")),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn structs() {
    check("structs", &["point", "meters", "account", "trip", "plain"]);
}

#[test]
fn enums() {
    check("enums", &["ident", "num", "eof", "tokens", "glyphs", "tagged", "by_ref"]);
}

#[test]
fn items() {
    check("items", &["map_str", "map_int", "stack", "empty", "raw", "inline_int", "inline_str", "rgb"]);
}

#[test]
fn deps() {
    check("deps", &["temp", "pair", "flag"]);
}

/// Non-ASCII summary text under an ASCII-only locale (`LC_ALL=C`, common in CI and
/// containers): GDB would raise UnicodeEncodeError, so the runtime escapes Rust-style.
/// LLDB writes UTF-8 regardless of locale.
#[test]
fn non_ascii_summary_in_ascii_locale() {
    build("dev");
    let bin = binary("deps", "dev");
    for dbg in debuggers() {
        let text = sections(&run_with_locale(&dbg, &bin, &["temp"], "C"), &["temp"])
            .unwrap_or_else(|e| panic!("{}: {e}", dbg.id));
        let expected = match dbg.kind {
            Kind::Gdb => "21.5\\u{b0}C",
            Kind::Lldb => "21.5\u{b0}C",
        };
        assert!(text.contains(expected), "{}: expected `{expected}` in:\n{text}", dbg.id);
    }
}
