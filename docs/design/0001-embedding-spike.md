# 0001: How descriptors get into the binary

**Status:** implemented in 0.1.0. This note records why the design is what it is; the current
contract is `docs/internal/schema-v1.md`.
**Measured on:** rustc 1.97, GDB 15.1, LLDB 18.1 / 20.1 / 22.1 (CodeLLDB), Ubuntu 24.04 x86_64.

## Decision

Each `#[derive(Debuggable)]` emits one `#[used]` static into a link section. The static holds a
small JSON description of the type. The `debuggable` crate adds one more entry: the GDB runtime
that renders those descriptions.

- **ELF (Linux):** the section is `.debug_gdb_scripts`, as GDB "inline script" entries (prefix
  byte `0x04`). GDB runs them itself once the binary is under `auto-load safe-path`. This is the
  same mechanism rustc uses for `#[debugger_visualizer(gdb_script_file = ...)]`.
- **LLDB** can't load anything from a binary, so `cargo debuggable setup` installs a loader that
  reads the **same section** and registers formatters lazily. Both debuggers work from one source
  of truth.
- **Mach-O (macOS):** `.debug_gdb_scripts` is not a valid section name there, so entries go to
  `__DATA,__debuggable`, read by the LLDB loader.
- **Windows and other targets:** nothing is emitted yet.

## Alternatives considered

| | Chosen: descriptors + shared runtime | Python script per type | Separate data-only section | `build.rs` + `#[debugger_visualizer]` |
|---|---|---|---|---|
| Library author | derive only | derive only | derive only | derive, a `build.rs`, and a crate attribute |
| GDB | works once the binary is trusted | same | needs a loader too | same as chosen |
| LLDB | via the installed loader | no story | via a loader | no story |
| Single source for both debuggers | yes | no | yes | no |

The `build.rs` approach worked, but every library author would need a build script, and the
build script can't easily discover which types are annotated. `#[debugger_visualizer]` also only
accepts a literal path, not `concat!(env!("OUT_DIR"), ...)`.

## Findings that shaped the implementation

- **Linking:** entries survive dev, release, thin LTO and fat LTO (`codegen-units = 1`) with
  `--gc-sections`, including entries from dependencies, from types with no functions, and from
  generic types only instantiated downstream. They also survive `strip`, because the section is
  allocated.
- **Names:** GDB runs each entry once per name, so names are unique per type:
  `debuggable-v1-<module path>::<Type>@<crate version>`. The module path comes from
  `module_path!()`, because a proc macro can't know it.
- **Generics:** DWARF names include the arguments (`app::SlotMap<u8, String>`), so one descriptor
  matches every instantiation by its path prefix.
- **`unsafe_code`:** `#[link_section]` trips the lint, but not when it comes from a macro in another
  crate. The attribute therefore lives in a `macro_rules!` in `debuggable`, and crates with
  `#![forbid(unsafe_code)]` still compile. Emitting `#[allow(unsafe_code)]` instead would be a hard
  error (E0453) under `forbid`.
- **Compile time:** building entry bytes in a `const fn` loop cost about 4.5 ms per type without
  incremental compilation. Entries are now one `concat!` string, about 1 ms per type
  (`tools/bench-compile-time.py` guards this in CI).
- **Enums in GDB:** values reached through arrays arrive with every variant listed, so the runtime
  re-reads them through their address before choosing the active variant.
- **Enums in LLDB:** LLDB 20 and earlier truncate variant discriminants to 32 bits in member names
  (`$variant$0` for a niche of `0x8000000000000000`); LLDB 22 does not. The loader accepts an
  exact match or a low-32-bit match.
- **Optimized builds:** every read of target memory is guarded, so an optimized-out field shows
  `<optimized out>` instead of aborting the whole value.
- **CodeLLDB** enables its own `Rust` formatter category after `initCommands`, which would shadow
  ours. The loader is imported from `preRunCommands` instead, and keeps its category first.

## Costs and opt-out

About 300 bytes per derived type, plus the compressed GDB runtime (about 6 KB) once per Linux
binary. This data is present in release builds too, because no `cfg` tells a crate whether debug
info is enabled. Building with `RUSTFLAGS="--cfg debuggable_disable"` removes all of it.

## Known limitations

- Types defined inside a function body: their DWARF path includes the function, which
  `module_path!()` can't see. Debuggers don't match them; `cargo debuggable doctor` reports them.
- Two semver-incompatible versions of one crate produce identical DWARF type names, so debuggers
  can't tell their types apart. Both descriptors load.
- A `staticlib` linked by a C toolchain is untested; `#[used]` statics may need whole-archive linking.
