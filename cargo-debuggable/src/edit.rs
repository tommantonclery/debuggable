//! Editing other programs' config files, safely.
//!
//! Every change lives in a marked block, so it can be updated or removed exactly:
//!
//! ```text
//! # >>> debuggable (managed by `cargo debuggable setup`; undo with `--remove`) >>>
//! ...
//! # <<< debuggable <<<
//! ```
//!
//! Removing the block restores the file byte-for-byte. Text outside the block is never
//! changed. All functions here are pure: they take and return file contents.

/// Comment style of the file being edited.
#[derive(Clone, Copy)]
pub enum Comment {
    /// `#` (gdbinit, .lldbinit)
    Hash,
    /// `//` (VS Code settings.json, which is JSON with comments)
    Slash,
}

impl Comment {
    fn prefix(self) -> &'static str {
        match self {
            Comment::Hash => "#",
            Comment::Slash => "//",
        }
    }
}

const BEGIN: &str = ">>> debuggable (managed by `cargo debuggable setup`; undo with `cargo debuggable setup --remove`) >>>";
const END: &str = "<<< debuggable <<<";

fn is_begin(line: &str, c: Comment) -> bool {
    let t = line.trim_start();
    t.starts_with(c.prefix()) && t.contains(">>> debuggable")
}

fn is_end(line: &str, c: Comment) -> bool {
    let t = line.trim_start();
    t.starts_with(c.prefix()) && t.contains("<<< debuggable <<<")
}

/// The lines inside our block (without markers and indentation), if the block exists.
pub fn block_lines(text: &str, c: Comment) -> Option<Vec<String>> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if is_begin(line, c) {
            inside = true;
        } else if is_end(line, c) {
            return Some(out);
        } else if inside {
            out.push(line.trim().to_string());
        }
    }
    None
}

/// `text` with our block (markers included) removed; everything else untouched.
pub fn remove_block(text: &str, c: Comment) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        if !inside && is_begin(line, c) {
            inside = true;
        } else if inside {
            if is_end(line, c) {
                inside = false;
            }
        } else {
            out.push_str(line);
        }
    }
    out
}

fn block(lines: &[String], c: Comment, indent: &str) -> String {
    let mut b = format!("{indent}{} {BEGIN}\n", c.prefix());
    for l in lines {
        b.push_str(indent);
        b.push_str(l);
        b.push('\n');
    }
    b.push_str(&format!("{indent}{} {END}\n", c.prefix()));
    b
}

/// gdbinit / .lldbinit: replace our block with `lines`, appended at the end of the file.
/// An empty `lines` removes the block.
pub fn set_hash_block(text: &str, lines: &[String]) -> String {
    let mut out = remove_block(text, Comment::Hash);
    if lines.is_empty() {
        return out;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&block(lines, Comment::Hash, ""));
    out
}

// ---- VS Code settings.json (JSONC) -------------------------------------------------------

/// What a JSONC scan found at the top level of the settings object.
struct Scan {
    /// Byte index of the top-level object's closing `}`.
    close: usize,
    /// Last significant (non-space, non-comment) byte before `close`.
    last_significant: u8,
    /// For `key`: byte index just after the `[` of its array value, and the first
    /// significant byte after that `[`.
    array: Option<(usize, u8)>,
}

/// Minimal JSONC scanner: strings, escapes, `//` and `/* */` comments, nesting.
/// Enough to find positions; it does not validate the whole document.
fn scan(text: &str, key: &str) -> Result<Scan, String> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut last_sig = 0u8;
    let mut last_string: Option<(usize, usize)> = None; // (start, end) of the last string at depth 1
    let mut want_array_for_key = false;
    let mut array = None;
    let mut seen_open = false;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'"' => {
                if depth == 1 && want_array_for_key {
                    return Err(format!("`{key}` is set to something other than a list"));
                }
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                if depth == 1 {
                    last_string = Some((start + 1, i));
                }
                last_sig = b'"';
            }
            b'{' | b'[' => {
                if b[i] == b'{' && depth == 1 && want_array_for_key {
                    return Err(format!("`{key}` is set to something other than a list"));
                }
                if depth == 0 {
                    if b[i] == b'[' {
                        return Err("settings must be a JSON object, not a list".into());
                    }
                    seen_open = true;
                }
                depth += 1;
                if b[i] == b'[' && depth == 2 && want_array_for_key {
                    let after = i + 1;
                    let next = text[after..]
                        .bytes()
                        .find(|c| !c.is_ascii_whitespace())
                        .unwrap_or(b']');
                    array = Some((after, next));
                    want_array_for_key = false;
                }
                last_sig = b[i];
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Err("unbalanced brackets".into());
                }
                depth -= 1;
                if depth == 0 {
                    return Ok(Scan { close: i, last_significant: last_sig, array });
                }
                last_sig = b[i];
            }
            b':' if depth == 1 => {
                if let Some((s, e)) = last_string {
                    if &text[s..e] == key {
                        want_array_for_key = true;
                    }
                }
                last_sig = b':';
            }
            c if c.is_ascii_whitespace() => {}
            c => {
                if depth == 1 && want_array_for_key && c != b'[' {
                    return Err(format!("`{key}` is set to something other than a list"));
                }
                last_sig = c;
            }
        }
        i += 1;
    }
    if seen_open {
        Err("unterminated object".into())
    } else {
        Err("no top-level object".into())
    }
}

/// Outcome of planning a settings.json edit.
#[derive(Debug, PartialEq)]
pub enum JsonEdit {
    /// The new file contents.
    Write(String),
    /// Already imports a `debuggable_lldb.py` outside our block (set up by hand): leave it.
    AlreadyManual,
}

/// Make VS Code settings run `command` in CodeLLDB's `lldb.launch.preRunCommands`.
/// Works on a missing/empty file, a file without the key (adds a member), and a file
/// whose own list already exists (adds our element to it). `None` removes our block.
pub fn set_vscode_command(text: &str, command: Option<&str>) -> Result<JsonEdit, String> {
    const KEY: &str = "lldb.launch.preRunCommands";
    let base = remove_block(text, Comment::Slash);
    let command = match command {
        None => return Ok(JsonEdit::Write(base)),
        Some(c) => c,
    };
    if base.contains("debuggable_lldb.py") {
        return Ok(JsonEdit::AlreadyManual);
    }
    let quoted = json_string(command);
    if base.trim().is_empty() {
        let body = block(&[format!("\"{KEY}\": [{quoted}]")], Comment::Slash, "    ");
        return Ok(JsonEdit::Write(format!("{{\n{body}}}\n")));
    }
    let s = scan(&base, KEY)?;
    let out = match s.array {
        // The user's own list exists: our element goes first, followed by a comma unless
        // the list is empty.
        Some((after_bracket, next)) => {
            let comma = if next == b']' { "" } else { "," };
            insert_lines(&base, after_bracket, &block(&[format!("{quoted}{comma}")], Comment::Slash, "        "))
        }
        // No such key: add a member before the closing brace, with a leading comma after
        // an existing member that has no trailing comma of its own.
        None => {
            let comma = if matches!(s.last_significant, b'{' | b',') { "" } else { ", " };
            let line_start = base[..s.close].rfind('\n').map_or(0, |i| i + 1);
            let at = if base[line_start..s.close].trim().is_empty() { line_start } else { s.close };
            insert_lines(&base, at, &block(&[format!("{comma}\"{KEY}\": [{quoted}]")], Comment::Slash, "    "))
        }
    };
    Ok(JsonEdit::Write(out))
}

/// Insert `block` (whole lines) at byte `at`. If `at` is at the start of a line (or right
/// before a line break), the block occupies whole lines and removing it later restores the
/// file exactly; otherwise line breaks are added around it.
fn insert_lines(text: &str, at: usize, block: &str) -> String {
    let mut at = at;
    // Right after `[`/`{` with only spaces before a line break: insert after that break.
    let rest = &text[at..];
    let spaces = rest.len() - rest.trim_start_matches([' ', '\t', '\r']).len();
    if rest[spaces..].starts_with('\n') {
        at += spaces + 1;
    }
    let at_line_start = at == 0 || text[..at].ends_with('\n');
    let mut out = String::with_capacity(text.len() + block.len() + 2);
    out.push_str(&text[..at]);
    if !at_line_start {
        out.push('\n');
    }
    out.push_str(block);
    out.push_str(&text[at..]);
    out
}

fn json_string(s: &str) -> String {
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            c if (c as u32) < 0x20 => q.push_str(&format!("\\u{:04x}", c as u32)),
            c => q.push(c),
        }
    }
    q.push('"');
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "command script import /home/u/.local/share/debuggable/lldb/debuggable_lldb.py";

    fn write(r: Result<JsonEdit, String>) -> String {
        match r.unwrap() {
            JsonEdit::Write(s) => s,
            other => panic!("expected a write, got {other:?}"),
        }
    }

    /// Strip `//` comment lines and check the rest parses as plain JSON-ish structure.
    fn assert_valid_jsonc(text: &str) {
        let without_comments: String =
            text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        assert!(scan(&without_comments, "").is_ok(), "not a balanced object:\n{text}");
        assert!(!without_comments.contains(",,"), "double comma:\n{text}");
    }

    #[test]
    fn hash_block_add_update_remove_roundtrip() {
        let original = "set history save on\n# user comment\n";
        let a = set_hash_block(original, &["add-auto-load-safe-path /p/target".into()]);
        assert!(a.starts_with(original));
        assert_eq!(block_lines(&a, Comment::Hash).unwrap(), vec!["add-auto-load-safe-path /p/target"]);
        let b = set_hash_block(&a, &["add-auto-load-safe-path /p/target".into(), "add-auto-load-safe-path /q/target".into()]);
        assert_eq!(block_lines(&b, Comment::Hash).unwrap().len(), 2);
        assert_eq!(set_hash_block(&b, &b_lines(&b)), b, "idempotent");
        assert_eq!(set_hash_block(&b, &[]), original, "remove restores the file exactly");
        // file without trailing newline
        let c = set_hash_block("source x", &["y".into()]);
        assert!(c.starts_with("source x\n# >>> debuggable"));
    }

    fn b_lines(t: &str) -> Vec<String> {
        block_lines(t, Comment::Hash).unwrap()
    }

    #[test]
    fn vscode_missing_or_empty_file() {
        for original in ["", "  \n"] {
            let out = write(set_vscode_command(original, Some(CMD)));
            assert_valid_jsonc(&out);
            assert!(out.contains("\"lldb.launch.preRunCommands\": [\"command script import"));
        }
    }

    #[test]
    fn vscode_adds_member_with_and_without_comma() {
        let cases = [
            "{\n    \"editor.fontSize\": 14\n}\n",
            "{\n    \"editor.fontSize\": 14,\n}\n",
            "{\n    // only a comment\n}\n",
            "{}",
            "{\n    \"a\": { \"b\": [1, 2] }, /* note */ \"c\": \"}\" // tricky\n}\n",
        ];
        for original in cases {
            let out = write(set_vscode_command(original, Some(CMD)));
            assert_valid_jsonc(&out);
            assert!(out.contains("lldb.launch.preRunCommands"));
            let restored = write(set_vscode_command(&out, None));
            if original.contains('\n') {
                assert_eq!(restored, original, "remove restores exactly:\n{out}");
            } else {
                let squash = |t: &str| t.split_whitespace().collect::<String>();
                assert_eq!(squash(&restored), squash(original), "remove restores the meaning:\n{out}");
            }
            assert_eq!(write(set_vscode_command(&out, Some(CMD))), out, "idempotent");
        }
    }

    #[test]
    fn vscode_merges_into_the_users_own_list() {
        let original = "{\n    \"lldb.launch.preRunCommands\": [\n        \"settings set target.x 1\"\n    ]\n}\n";
        let out = write(set_vscode_command(original, Some(CMD)));
        assert_valid_jsonc(&out);
        assert!(out.contains("settings set target.x 1"), "user's command kept");
        let ours = out.find("debuggable_lldb.py").unwrap();
        let theirs = out.find("settings set").unwrap();
        assert!(ours < theirs && out[ours..theirs].contains(','), "ours first, comma-separated:\n{out}");
        assert_eq!(write(set_vscode_command(&out, None)), original, "remove restores exactly");
        // empty list: no comma
        let empty = "{ \"lldb.launch.preRunCommands\": [] }";
        let out = write(set_vscode_command(empty, Some(CMD)));
        assert_valid_jsonc(&out);
        assert!(!out.contains("py\","), "no trailing comma in an otherwise empty list:\n{out}");
    }

    #[test]
    fn vscode_manual_setup_is_left_alone() {
        // the user's file from the Phase 5 experiment, configured by hand
        let manual = "{\n    \"lldb.launch.preRunCommands\": [\n    \"command script import /home/u/src/debuggable/cargo-debuggable/assets/debuggable_lldb.py\"\n    ]\n}\n";
        assert_eq!(set_vscode_command(manual, Some(CMD)).unwrap(), JsonEdit::AlreadyManual);
    }

    #[test]
    fn vscode_rejects_what_it_cannot_edit_safely() {
        assert!(set_vscode_command("{ \"lldb.launch.preRunCommands\": \"oops\" }", Some(CMD)).is_err());
        assert!(set_vscode_command("{ \"lldb.launch.preRunCommands\": { \"x\": 1 } }", Some(CMD)).is_err());
        assert!(set_vscode_command("{ \"lldb.launch.preRunCommands\": 3 }", Some(CMD)).is_err());
        assert!(set_vscode_command("{ \"a\": 1", Some(CMD)).is_err());
        assert!(set_vscode_command("[1, 2]", Some(CMD)).is_err());
    }
}
