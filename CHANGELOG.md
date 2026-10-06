# Changelog

All three crates (`debuggable`, `debuggable-derive`, `cargo-debuggable`) are released together
with the same version. This project follows [Semantic Versioning](https://semver.org).

## 0.1.0 — 2026-10-07

First release.

### `debuggable`

- `#[derive(Debuggable)]` for structs (named, tuple and unit) and enums, including generic
  types (one description covers every instantiation).
- Attributes:
  - `summary = "..."` on structs and enum variants, with `{field}` / `{0}` placeholders.
  - `hide` and `rename = "..."` on fields. `PhantomData` fields are hidden automatically.
  - `items` on one field, with optional `len = "field"`: shows the elements of a `Vec<T>`, an
    inline array `[T; N]`, or a `*const T` / `*mut T` / `NonNull<T>` with a length. Elements
    wrapped in `MaybeUninit<T>` or `ManuallyDrop<T>` are shown as `T`.
- Compile errors point at the attribute, with "did you mean" suggestions for misspelled fields
  and options.
- GDB on Linux loads the visualizers from the binary automatically (once its directory is
  trusted). LLDB and VS Code (CodeLLDB) on Linux and macOS use the loader installed by
  `cargo debuggable setup`.
- Cost: about 300 bytes per derived type plus a 4.4 KB GDB runtime once per binary, as data
  only debuggers read; about 1 ms of compile time per derived type. No `unsafe` in your crate,
  and `#![forbid(unsafe_code)]` crates work.
- `--cfg debuggable_disable` emits nothing.
- Nothing is emitted on Windows yet.
- MSRV: Rust 1.75.

### `cargo-debuggable`

- `cargo debuggable setup [--gdb] [--lldb] [--vscode] [--dry-run] [--remove]` configures GDB,
  LLDB and CodeLLDB through marked, removable blocks, with a backup before the first change to
  each file.
- `cargo debuggable doctor [BINARY]` checks the installed debuggers, their configuration, and
  what a binary embeds.

### `debuggable-derive`

- Implementation of the derive. Use it through `debuggable`.
