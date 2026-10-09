# Changelog

All three crates (`debuggable`, `debuggable-derive`, `cargo-debuggable`) are released together
with the same version. This project follows [Semantic Versioning](https://semver.org).

## 0.1.3 — 2026-10-09

### `debuggable`

- Inline-or-heap collections (smallvec, tinyvec): `items = "path"` on the struct or enum, once
  per place the elements can be, with an optional `len = "path"`. Path segments can name an enum
  variant; the first place whose paths exist in the value is shown. GDB runtime 1.7.
- `{#}` in summaries: the number of elements shown, for any type with `items`.

### `cargo-debuggable`

- LLDB loader 1.5: supports `items = "path"` alternatives and `{#}`. Run `cargo debuggable setup`
  again after upgrading.

## 0.1.2 — 2026-10-07

### `debuggable`

- `items` takes two new options for slot collections (slab-, arena- and slot-map-style types):
  `only = "Occupied"` or `only = "version & 1"` keeps matching elements, and `value = "path"`
  shows a field of each instead of the whole slot. Kept elements keep their index. GDB
  runtime 1.6.

### `cargo-debuggable`

- LLDB loader 1.4: supports `only` and `value`. Run `cargo debuggable setup` again after
  upgrading.
- Fixed: in LLDB, variant summaries on generic enums (`enum Msg<T>`) were never used; the
  variant showed as `Data<unsigned int>`.

### `debuggable-derive`

- "Did you mean" suggestions now also catch two swapped letters (`onyl` → `only`).

## 0.1.1 — 2026-10-07

### `debuggable`

- New `text` field attribute: shows a byte buffer (`[u8; N]`, `[MaybeUninit<u8>; N]`,
  `Vec<u8>`, or a byte pointer with `len`) as a string, `"hello"`. Escapes follow Rust's
  `{:?}`, and invalid UTF-8 shows as `\xNN`. Works with `len` and `hide`, so an inline string
  type can display exactly like `String`. GDB runtime 1.5.

### `cargo-debuggable`

- LLDB loader 1.3: supports `text`. Run `cargo debuggable setup` again after upgrading.
- `doctor <binary>` checks each described type against the binary's debug info (Linux): it
  warns about types debuggers can't match, such as types defined inside a function, and about
  debug info without types (`debug = "line-tables-only"`).

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
