# debugview: embedding-mechanism spike

> **Renamed 2026-10-06:** the project is now **`debuggable`** (`debuggable`, `debuggable-derive`,
> `cargo-debuggable`). It was renamed partly to avoid confusion with Sysinternals DebugView.
> The spike code below keeps the old names, and the findings are unaffected.

**Status:** spike complete. A decision is needed (§6) before any crate scaffolding.
**Environment:** rustc 1.97.0, GDB 15.1, LLDB 18.1.3 and 20.1.2, Ubuntu 24.04 x86_64.
Every claim below links to a reproducible proof in `proto/`.

## 1. Recommendation (TL;DR)

**Use a hybrid of A and B. Call it "A as the carrier, B as the format".**

- Each `#[derive(DebugView)]` emits one `#[used]` static into `.debug_gdb_scripts` as an
  *inline-script* entry (prefix byte `0x04`). Its only payload is a JSON descriptor of the type,
  plus three lines of Python that put it in a queue.
- The `debugview` facade crate emits **one** shared runtime entry, which holds the actual GDB
  printer code. The queue makes load order irrelevant.
- **GDB** auto-loads everything once the binary is on `auto-load safe-path`. No files, no
  build script, and no aggregation step.
- **LLDB** can't auto-load from a binary, so a loader installed by `cargo debugview` reads
  **the same section** and registers formatters lazily through a recognizer function.
  That gives one source of truth for both debuggers, which is B's main benefit, without a second section.
- Mach-O uses `__DATA,__debugview` (LLDB only). Windows and other targets emit nothing in v0.1.

C works and stays as the fallback. It costs every library author a `build.rs`, and the
source-discovery problem remains.

This is the same mechanism rustc uses itself: `#[debugger_visualizer(gdb_script_file)]` is
implemented by embedding the file as an inline `0x04` entry named `pretty-printer-<crate>-0`
in the same section (observed with `readelf`).

## 2. Constraints, verified

| # | Brief's claim | Result |
|---|---|---|
| 1 | Path may be a literal or a macro | **Literal only.** `concat!(env!("OUT_DIR"), …)` is rejected with "macro calls are not allowed here", and so is a `$p:expr` from `macro_rules!`. Absolute literal paths work. A proc macro *can* read `OUT_DIR` at expansion time and emit the literal. That works, and rustc tracks the file in dep-info. |
| 2 | Natvis only on `-windows-msvc` | Confirmed (Reference). |
| 3 | GDB scripts are not auto-loaded | Confirmed. GDB prints "auto-loading has been declined" until the binary is on the safe-path. |
| 4 | LLDB is unsupported by the attribute | Confirmed. LLDB has no section auto-load, so we need our own loader. |
| 5 | Derive can't emit `#![…]` | Doesn't matter for A/B, because each derive's static stands alone. |
| 6 | No writing files from proc macros | Not needed by A/B. C writes from `build.rs`, which is allowed. |
| 7 | Compile-time cost | The derive emits **one line**, `::debugview::__entry!(…)`. The byte assembly happens in a `macro_rules!` and in const eval. Not benchmarked yet. |

**New constraints the spike found:**

- **`#[link_section]` trips the `unsafe_code` lint**, and edition 2024 spells it
  `#[unsafe(link_section)]`. When the attribute comes from a `macro_rules!` in our facade
  (or directly from the derive), a user crate with `#![forbid(unsafe_code)]` still compiles in
  both editions, because the lint doesn't fire on tokens from external macros. *We must not
  emit `#[allow(unsafe_code)]`*, since that line is itself a hard error (E0453) under
  `forbid`. We rely on lint behaviour for macro expansions here, so this gets a CI canary.
- **On Apple targets `.debug_gdb_scripts` is a compile error** ("invalid Mach-O section specifier").
  The section name has to be `cfg`-selected per target.
- **The section survives `strip`** (and `strip --strip-debug`) because it is `ALLOC`. It also
  stays in release builds with `debug = false`. rustc drops its own entry in that profile; we
  can't, because no `cfg` exposes "debuginfo on". See §5.

## 3. What each experiment showed

### A: per-type `.debug_gdb_scripts` entries (`proto/run-gdb.sh`)

- **Linking:** entries from the binary crate, from a dependency, from a *pure-data* type with
  no functions, from a generic type that is only monomorphized downstream, and from a type the
  app never uses all survived in **dev, release, thin LTO, and fat LTO with cgu=1**, under
  `--gc-sections`. On ELF, `#[used]` produces `SHF_GNU_RETAIN` (`AR` flags). A crate that the
  source never mentions isn't linked at all, which is fine because no values of its types can exist.
- **GDB executes each entry once by name.** The names are `debugview-v1-<module_path>::<Type>@<pkg version>`,
  built with `concat!(module_path!(), …)` because a proc macro can't know its own module path.
- **Generics:** DWARF names look like `app::SlotMap<app::Key, alloc::string::String>`, so a single
  `^path(<.*>)?$` regex per definition covers every instantiation. Verified with two instantiations.
- **Enums:** GDB resolves the active variant itself. Gotcha: **array elements reach printers
  unresolved** (every variant listed), which made `[Eof, Eof, Eof]` print. The fix is to re-read
  through `val.address`, and it's verified.
- **Optimized builds:** optimized-out values have to be guarded at *every* `gdb.Value` operation,
  including lazy fetches and pointer arithmetic. Without that, printing aborts mid-value. The
  prototype now degrades to `<optimized out>`/`<unavailable>`. Remaining LTO inaccuracies (for
  example `slots.len` reads 1) match GDB's raw view, so they come from the debug info, not from us.
- **Std types:** plain `gdb` shows `String` raw. `rust-gdb` (rustc's std printers) shows `"alpha"`.
  Our setup story should load rustc's std printers too. That stays in our lane: we only load them.

Output (rust-gdb, debug build):
```
$1 = 2 items = { len = 2, [0] = v1 Some("alpha") = {…}, [1] = v1 Some("beta") = {…} }
$3 = [Ident("foo") = { name = "foo" }, Num(42) = { 0 = 42 }, Eof]
```

### B: neutral descriptors plus an LLDB loader (`proto/run-lldb.sh`, `proto/debugview_lldb.py`)

- LLDB reads the section through `SBModule.FindSection(...).GetSectionData()`, so we don't
  need a second section or a sidecar file.
- The loader uses `--recognizer-function` (present in LLDB 18 and 20), so it matches lazily and
  parses each module once.
- **LLDB 20 + rustc std formatters + debugview:** summaries, `hide`, `items`, generics, nested
  modules, and enums are all correct: `[Ident("foo"), Num(42), Eof]`.
- **Upstream bug 1 (affects our enums):** LLDB **truncates variant discriminants to 32 bits**.
  It shows `$variant$1114112` for a `char` niche (correct) but `$variant$0` for the niche
  `0x8000000000000000`. Because of this, **rustc's own LLDB formatter misprints niche enums**
  on LLDB 20 (`Num(42)` shows as `Ident{name:""}`). Our prototype matches on the low 32 bits,
  which is right except when a niche field's low bits collide. That's rare but possible
  (for example a `Vec` capacity ≥ 2³² with zero low bits). I haven't verified whether LLDB 21+
  has fixed it.
- **Upstream bug 2:** rustc 1.97's std formatters crash on **LLDB 18** (Ubuntu 24.04's default)
  with `SBValue has no attribute GetSyntheticValue`. Our own types still render; std
  `Option`/`String` don't.
- Minor prototype bug: the first `GetSummary()` on a nested std value sometimes returns
  `<unavailable>`.

### C: build script plus a crate attribute (`proto/userC`)

Works, via `debugview::visualizers!()`: a proc macro reads `OUT_DIR` and emits
`#[debugger_visualizer(gdb_script_file = "/abs/out/debugview.py")]`. GDB loads it, and rustc
lists the file in dep-info. The open problems remain: discovering annotated types from
`build.rs`, and a mandatory `build.rs` for every library author.

### Cross-target object checks (`no_core` objects; full linking untested)

| Target | `.debug_gdb_scripts` | Alternative |
|---|---|---|
| x86_64 Linux (ELF) | OK, flags `AR` | n/a |
| aarch64 macOS (Mach-O) | **compile error** | `__DATA,__debugview` OK, symbol marked `[no dead strip]` |
| x86_64 windows-msvc (COFF) | compiles, no consumer | v0.1: emit nothing |

## 4. Comparison

| | **A+B hybrid (recommended)** | A alone (Python per type) | B alone (separate section) | C (build.rs) |
|---|---|---|---|---|
| Library-author setup | derive only | derive only | derive only | derive + `build.rs` + `visualizers!()` |
| End user, GDB | safe-path line (`cargo debugview setup` writes it) | same | must run the loader too | same as A |
| End user, LLDB | `cargo debugview setup` (loader in lldbinit / CodeLLDB `initCommands`) | no LLDB story | same as hybrid | no LLDB story |
| Platforms | ELF: GDB + LLDB. Mach-O: LLDB. Windows: none | ELF GDB | ELF + Mach-O | wherever `debugger_visualizer` works |
| LTO / gc / incremental | verified in all four profiles | same | same mechanism | relies on rustc's handling |
| Binary cost | about 0.3 KB per type + 5.5 KB runtime, also in non-debug builds | about 5 KB **per type** (estimate) if the runtime is duplicated | same as hybrid | small, only when a debugger script is embedded |
| Single source of truth | yes (JSON) | no (two code generators) | yes | no |
| Complexity | moderate: two Python runtimes plus a byte format | low | moderate | high (type discovery) |

## 5. Risks and open issues

1. **Size in non-debug release builds.** (Decision 2.)
2. **LLDB enum truncation.** (Decision 3.)
3. Two semver-incompatible versions of one crate produce identical DWARF type names. Debuggers
   can't tell them apart either. The entry names include `@version`, so both load, and we document it.
4. Types defined inside function bodies: `module_path!()` won't match their DWARF path. We
   probably refuse these with a compile error.
5. `staticlib` linked by a C toolchain: untested. `#[used]` may be dropped without whole-archive linking.
6. Untested so far: CodeLLDB's bundled LLDB (the download was blocked from the sandbox), real
   macOS linking and LLDB, and GDB < 15. All of these belong in the Docker/CI matrix.
7. **Prior art:** `dbgvis` (published 2026-09-17, about 13 downloads) is nightly-only and runs
   `Debug` inside the debuggee. Different philosophy, not a competitor for the static
   approach. **Names:** `debugview`, `debugview-derive`, `cargo-debugview`, and `debugview-contrib`
   are all free on crates.io as of 2026-10-06.

## 6. Decisions I need from you

1. **Embedding mechanism:** adopt the A+B hybrid? (Hard to reverse once descriptors ship.)
2. **Non-debug overhead:** (a) always emit, compressing the runtime to about 2 KB;
   (b) also add an opt-out `--cfg debugview_disable`; (c) leave the runtime out of the binary, so
   GDB also needs `cargo debugview setup` and only descriptors (about 0.2 KB per type) are embedded.
   I lean towards (a)+(b).
3. **LLDB enums:** ship the low-32-bit heuristic in v0.1, add exact discriminant tables from
   DWARF via `cargo debugview` (gimli) later, and report the truncation upstream. Or require exactness from day one?
4. **`items` semantics:** when `items` is present, should other non-hidden fields still show as children? The spike shows both.

### Proposed answers

1. **Adopt the A+B hybrid.** It's the only option where GDB works with nothing but the derive and
   LLDB works from the same data. To keep it reversible, put the schema version in the entry name
   (`debugview-v1-…`) and treat the descriptor schema as internal (`#[doc(hidden)]`, no stability promise).
2. **(a) + (b).** Compress the runtime with zlib and run it with `exec(zlib.decompress(...))`
   (about 2 KB). Trim the per-type boilerplate (target: about 200 B per type). Offer
   `RUSTFLAGS="--cfg debugview_disable"` as the opt-out. Reject (c), because it gives up
   GDB's zero-setup advantage. State the size cost plainly in the docs.
3. **Exact match first, low-32-bit fallback for LLDB ≤ 20.** Superseded by §7: LLDB 22 (CodeLLDB)
   is already fixed, so there's no upstream bug to file and no DWARF tables to build.
4. **Show the other visible fields too.** `items` adds a view and hides nothing. The items
   field's elements replace the raw field (remaining fields first, then `[0]`, `[1]`, …). The
   docs recommend `hide` for bookkeeping fields.

**Related syntax questions:**

- `len` takes a field name, `#[debug_view(items, len = "len")]`. Not `self.len`, which would look
  like an expression but only accept field paths. A bare identifier can be accepted later.
- `PhantomData` fields are hidden automatically (matched on the type's last path segment).
- Types defined inside function bodies are documented as unsupported, because the derive can't
  detect them. `cargo debugview doctor` reports descriptors whose type never matches anything in the debug info.

## 7. Phase 0 results (2026-10-06)

**CodeLLDB (VS Code + WSL2 Ubuntu 24.04, bundled `lldb version 22.1.8-codelldb`): PASS.**
`m = 2 items`, `m2 = 1 items`, `toks = {Ident("foo"), Num(42), Eof}`, `t = Num(42)`,
`n = nested size=0`. No manual steps once the two fixes below were applied.

What we learned:

1. **The LLDB discriminant truncation is fixed by 22.1.8.** Variants are named with the full value
   (`$variant$9223372036854775808`). It was still present in 20.1.2. The exact fix version (21 or 22)
   still needs finding in the LLVM history; there's no need to file a new bug.
2. **The loader's enum match must accept both forms:** `int(suffix) in (d, d & 0xFFFFFFFF)`.
   Low-32-only matching mis-resolved every niche enum on LLDB 22 (all `Ident`, plus a
   "String pointer is null" error). The Phase 3 harness needs fixtures on both LLDB ≤20 and ≥22.
3. **Category priority:** CodeLLDB enables its `Rust` category *after* `initCommands`, which puts it
   ahead of `debugview`. rustc's summaries then win, while our synthetic children still apply,
   which is a confusing half-working state. Importing the loader in **`preRunCommands`** fixes the
   order. `cargo debugview setup` must write `preRunCommands`, and `doctor` should check the category order.
4. CodeLLDB doesn't show Python `print` output in the Debug Console. `doctor` shouldn't rely on it.

Still open: macOS linking (`__DATA,__debugview`), deferred to a GitHub Actions `macos-latest` job.

## Layout

```
proto/
  debugview/            facade: Entry type, __entry! macro, shared GDB runtime (gdb_rt.py)
  debugview-derive/     zero-dependency spike derive (type name only; descriptors hand-written in app)
  app/                  fixture: SlotMap<K,V>, Slot<V>, Token, nested-module generic
  user2021/ user2024/   #![forbid(unsafe_code)] crates using the derive
  userC/                approach C proof
  debugview_lldb.py     LLDB loader reading the same section
  run-gdb.sh  run-lldb.sh
```
