# How it works

Rust supports debugger visualizers through `#[debugger_visualizer]`, but the attribute only goes
on a module or crate, takes a hand-written script file, and doesn't cover LLDB. `debuggable`
works differently.

## 1. The derive embeds a description

`#[derive(Debuggable)]` validates the attributes and expands to one macro call. That call puts a
static byte array in a dedicated link section:
- `.debug_gdb_scripts` on Linux (ELF);
- `__DATA,__debuggable` on macOS (Mach-O).

The bytes are a small JSON description of the type: its path, which fields to hide or rename,
the summary template, and where the items are. They're wrapped in the format GDB uses for inline
scripts. Nothing in your program reads these bytes or runs because of them.

`.debug_gdb_scripts` is the same mechanism rustc itself uses to embed scripts. Entries survive
linking from any crate, in any profile, including LTO, and survive `strip`.

## 2. GDB: an embedded runtime

The `debuggable` crate adds one more entry to the same section: a 5 KB compressed Python runtime
that renders the descriptions. GDB runs the scripts in that section for any binary under its
*auto-load safe-path*, so on Linux the only setup is trusting your build directory, which
`cargo debuggable setup` does.

If a binary contains several versions of `debuggable`, the newest runtime takes over, and it
understands every description in the binary.

## 3. LLDB: a loader that reads the same data

LLDB can't load scripts from a binary. `cargo debuggable setup` installs a small loader instead,
imported from `~/.lldbinit` for the command line and from VS Code's settings for CodeLLDB. The
loader reads the *same* descriptions straight out of the binary, so one description serves both
debuggers.

## Why descriptions rather than generated scripts

Keeping the per-type data declarative means:
- each derived type costs about 300 bytes instead of a full script;
- the rendering logic lives in one place per debugger, where it can be tested and fixed
  without recompiling anyone's crate;
- the same data can later drive other debuggers (Natvis on Windows is the obvious next one).

The exact format is in [`docs/internal/schema-v1.md`](internal/schema-v1.md). The measurements
and experiments behind these choices are in
[`docs/design/0001-embedding-spike.md`](design/0001-embedding-spike.md).

## Testing

Every commit runs real debuggers (GDB 15, LLDB 18, 20 and 22, and Apple LLDB) against fixture
programs in four build profiles, and compares their output to reviewed snapshots. See
[`tests/README.md`](../tests/README.md).
