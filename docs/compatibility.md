# Compatibility

Everything marked **CI** is tested on every commit by snapshot tests that run the real debugger
and compare its output. The fixtures cover summaries, hidden and renamed fields, tuple fields,
generics, enums (tagged and niche layouts, arrays, references), both kinds of `items`, nested
summaries, types from dependency crates, and crates with `#![forbid(unsafe_code)]` on editions
2021 and 2024. Each runs in **dev, release, thin-LTO and fat-LTO** builds.

## Debuggers

| Platform | Debugger | Status | Notes |
|---|---|---|---|
| Linux x86-64 | GDB 15 | **CI** | Plain `gdb` and `rust-gdb`. |
| Linux x86-64 | LLDB 18 | **CI** | rustc's own formatters fail on std enums with LLDB 18 (see [troubleshooting](troubleshooting.md)); `debuggable` types are unaffected. |
| Linux x86-64 | LLDB 20 | **CI** | |
| Linux x86-64 | LLDB 22 | **CI** | The version CodeLLDB bundles. |
| Linux x86-64 | VS Code + CodeLLDB (bundled LLDB 22.1.8) | tested by hand | Under WSL 2. Configured by `cargo debuggable setup --vscode`. |
| macOS (Apple silicon) | Apple LLDB (lldb-2100) | **CI** | GitHub's `macos-latest` runner. |
| macOS | VS Code + CodeLLDB | expected to work | Same loader and setting as on Linux; not yet tested by hand. |
| macOS | GDB | not supported | |
| Windows (MSVC) | Visual Studio, WinDbg, CodeLLDB | not supported yet | Nothing is emitted, so crates using `debuggable` still build and run normally. Natvis support is planned. |
| Windows | WSL 2 | as Linux | |
| Other Unix (FreeBSD, Android, ...) | GDB / LLDB | untested | The same ELF section is emitted as on Linux. |

Older debuggers (GDB < 15, LLDB < 18) are untested and may work. LLDB versions up to 20 shorten
enum discriminants in their debug info API; the loader accounts for this.

## Rust

- **MSRV: 1.75** for `debuggable`, `debuggable-derive` and `cargo-debuggable`, checked in CI.
- Debugger snapshots are taken with a pinned toolchain (`tests/fixtures/rust-toolchain.toml`),
  because rustc ships the std-type formatters that appear inside the output.
- The derive works in crates using editions 2021 and 2024.

## Build configurations

| | Status |
|---|---|
| dev, release (with `debug = true`), thin LTO, fat LTO | **CI**; identical debugger output in all four |
| `strip` / `strip --strip-debug` | **CI**: the entries survive (but stripping debug info leaves the debugger nothing to show) |
| `RUSTFLAGS="--cfg debuggable_disable"` | **CI**: emits nothing, with no warnings |
| `cdylib`, `dylib` | expected to work, untested |
| `staticlib` linked by a C toolchain | untested: the C linker may drop the entries |
