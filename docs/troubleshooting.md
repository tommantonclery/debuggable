# Troubleshooting

**Start here:** `cargo debuggable doctor target/debug/<your-binary>` checks your debuggers,
your configuration and the binary, and prints the fix for anything it finds. The sections below
explain each problem in more detail, by symptom.

## GDB shows the raw struct, not the summary

**Most likely: GDB didn't trust the binary.** GDB only runs scripts embedded in binaries that
are under its *auto-load safe-path*, and `debuggable`'s visualizers are such a script. Plain
`gdb` says so when it starts:

```text
warning: File ".../target/debug/my-app" auto-loading has been declined by your `auto-load safe-path' ...
```

Fix: run `cargo debuggable setup --gdb` in the project. It adds the project's `target/`
directory to the safe-path. Setup is per project on purpose: it trusts your own build output,
not every binary on your machine.

**Or: the binary has no `debuggable` entries.** `doctor` reports this. The causes are:
- no crate in the build derives `Debuggable`;
- the build used `RUSTFLAGS="--cfg debuggable_disable"`;
- the target is Windows, which isn't supported yet.

## GDB prints "Missing auto-load script ... gdb_load_rust_pretty_printers.py"

This comes from rustc, not from `debuggable`. Every Rust binary carries an entry for rustc's
printers of standard library types, and only `rust-gdb` knows where to find them. It's harmless.
Use `rust-gdb` instead of `gdb` to see `String`, `Vec` and other std types formatted too, inside
your summaries as well.

## In plain `gdb`, std types inside a summary look raw

For example `Ident(alloc::string::String { vec: {...} })` instead of `Ident("foo")`. Same cause
as above: use `rust-gdb`.

## VS Code shows `{x:1, y:2}`-style values instead of the summary

CodeLLDB isn't importing the `debuggable` loader. Run `cargo debuggable setup --vscode`. It sets
`lldb.launch.preRunCommands` in your VS Code settings: the machine settings under WSL or
remote development, your user settings otherwise. No `launch.json` changes are needed.

To confirm, run this in the Debug Console while stopped:

```text
debuggable-status
```

It reports the loader version, whether `debuggable` takes priority over rustc's formatters, and
how many described types each loaded module contains. If the command is unknown, the loader
isn't imported. (CodeLLDB does **not** read `~/.lldbinit`.)

## Command-line LLDB shows raw values

Run `cargo debuggable setup --lldb`. It imports the loader from `~/.lldbinit`, which both `lldb`
and `rust-lldb` read. `debuggable-status` works there too.

## Values show `<optimized out>` or `<unavailable>`

The value isn't available in the debug info, which is common in release builds. `debuggable`
shows what the debugger can read and marks the rest, rather than failing. Debug builds
(`cargo build`) give the most complete picture.

## A release build shows nothing at all

The binary probably has no debug info: the release profile leaves it out by default. Add this
to `Cargo.toml`:

```toml
[profile.release]
debug = true
```

## Tracebacks from `lldb_providers.py` in LLDB 18

This is rustc's own LLDB formatters failing on LLDB 18, Ubuntu 24.04's default: they call an API
LLDB 18 doesn't have. Standard library enums such as `Option` then show raw. `debuggable` types
are unaffected. Newer LLDB versions, including the one CodeLLDB bundles, don't have this problem.

## A summary shows `\u{b0}` instead of `°`

The debugger is running under an ASCII-only locale (`LC_ALL=C`, common in containers and CI).
GDB can't print the character there, so `debuggable` escapes it rather than failing. Use a
UTF-8 locale, for example `LC_ALL=C.UTF-8`.

## `doctor` says "universal (fat) macOS binary"

Check one architecture at a time:
`lipo -thin arm64 -output my-app-arm64 my-app && cargo debuggable doctor my-app-arm64`.

## Undoing setup

`cargo debuggable setup --remove` removes everything `setup` added: the marked blocks in your
config files, and the installed loader. Your files are restored exactly. Backups from the first
change are kept next to them as `*.debuggable.bak`.
