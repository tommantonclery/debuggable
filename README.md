# debuggable

**Make your Rust types readable in GDB and LLDB with one derive.**

[![CI](https://github.com/tommantonclery/debuggable/actions/workflows/ci.yml/badge.svg)](https://github.com/tommantonclery/debuggable/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/debuggable.svg)](https://crates.io/crates/debuggable)
[![docs.rs](https://docs.rs/debuggable/badge.svg)](https://docs.rs/debuggable)

Debuggers show the *representation* of your types: pointers, capacities, bookkeeping fields,
and slots that aren't in use. `debuggable` lets you describe what a value *means*, once, next to
the type, and every user of your crate sees that in GDB, LLDB and VS Code.

```text
(gdb) print buf                                              before
$1 = my_crate::RawBuf {ptr: core::ptr::non_null::NonNull<u16> {pointer: 0x5555555aded0}, len: 3, cap: 4}

(gdb) print buf                                              after
$1 = 3/4 = {len = 3, cap = 4, [0] = 10, [1] = 20, [2] = 30}
```

```text
(lldb) v map                                                 before
(my_crate::SlotMap<u8, alloc::string::String>) map = {
  slots = size=3 {
    [0] = {...}
    [1] = {...}
    [2] = {...}
  }
  free_head = 3
  len = 2
  _k =
}

(lldb) v map                                                 after
(my_crate::SlotMap<u8, alloc::string::String>) map = 2 items {
  [0] = v1 Some("alpha") {...}
  [1] = v3 Some("beta") {...}
}
```

The third slot is free (`len` is 2), so `debuggable` doesn't show it. (`{...}` marks collapsed
values, as in VS Code's variables view.)

## Make your crate debuggable in 5 minutes

**1. Add the dependency**

```sh
cargo add debuggable
```

**2. Describe your types**

```rust
use debuggable::Debuggable;
use std::marker::PhantomData;
use std::ptr::NonNull;

#[derive(Debuggable)]
#[debuggable(summary = "{len} items")]
pub struct SlotMap<K, V> {
    #[debuggable(items, len = "len")]   // show the live slots as elements
    slots: Vec<Slot<V>>,
    #[debuggable(hide)]                 // bookkeeping nobody wants to see
    free_head: u32,
    #[debuggable(hide)]
    len: u32,
    _k: PhantomData<K>,                 // PhantomData is hidden automatically
}

#[derive(Debuggable)]
#[debuggable(summary = "{len}/{cap}")]
pub struct RawBuf {
    #[debuggable(items, len = "len")]   // a raw pointer + length works too
    ptr: NonNull<u16>,
    len: usize,
    cap: usize,
}

#[derive(Debuggable)]
pub enum Token {
    #[debuggable(summary = "Ident({name})")]
    Ident { name: String },
    #[debuggable(summary = "Num({0})")]
    Num(i64),
    Eof,                                // no summary: shown as `Eof`
}

#[derive(Debuggable)]
#[debuggable(summary = "v{version} {value}")]
pub struct Slot<V> {
    value: Option<V>,
    #[debuggable(hide)]
    version: u32,
}
```

Mistakes are compile errors that point at the problem:

```text
error: summary refers to unknown field `lenght`; did you mean `length`?
 --> src/lib.rs:2:24
  |
2 | #[debuggable(summary = "{lenght} items")]
  |                        ^^^^^^^^^^^^^^^^
```

**3. Set up your debugger (once per machine; GDB once per project)**

```sh
cargo install cargo-debuggable
cargo debuggable setup --dry-run   # see what it would change
cargo debuggable setup
```

This configures whichever of GDB, LLDB and VS Code (CodeLLDB) you have. It only edits clearly
marked blocks in your config files, backs each file up first, and `cargo debuggable setup --remove`
undoes everything exactly.

**4. Debug as usual**: `rust-gdb`, `rust-lldb`, `lldb`, or F5 in VS Code with CodeLLDB.

**5. If something doesn't show up**

```sh
cargo debuggable doctor target/debug/my-app
```

It checks your debuggers, your configuration and the binary, and tells you the exact fix. See also
[troubleshooting](docs/troubleshooting.md).

## For library authors

Your users get the visualizers automatically: the descriptions are compiled into their binaries
along with your types. They only run `cargo debuggable setup` once, which they may have done
already for another crate.

What it costs them:

- **Binary size:** about 300 bytes per derived type, plus a 4 KB runtime once per Linux binary.
  This is data in a section the debugger reads; no code runs in your program. To leave it out,
  build with `RUSTFLAGS="--cfg debuggable_disable"`.
- **Compile time:** under 1 ms per derived type on rebuilds (0.4–0.8 ms measured; guarded in CI). The derive uses `syn` 3, which
  `serde`, `tokio`, `thiserror` and `clap` already pull in, so most projects compile nothing extra.
- **Code:** none. No `unsafe` is required in your crate, and it works under
  `#![forbid(unsafe_code)]`.
- **MSRV:** Rust 1.75.

## Attributes

| Attribute | On | Effect |
|---|---|---|
| `summary = "..."` | struct, enum variant | One-line summary. `{field}` (or `{0}` for tuple fields) inserts a field; `{{` and `}}` are literal braces. |
| `hide` | field | Hide the field. `PhantomData` fields are hidden automatically. |
| `rename = "..."` | field | Show the field under another name. |
| `items` | field | Show the field's elements in place of the field: a `Vec<T>`, or a `*const T`, `*mut T` or `NonNull<T>` together with `len`. |
| `len = "field"` | with `items` | The number of elements to show: for a `Vec`, at most this many; for a pointer, required. |

Full reference: [docs.rs/debuggable](https://docs.rs/debuggable).

## Support

| | GDB | LLDB | VS Code (CodeLLDB) |
|---|---|---|---|
| Linux (x86-64) | 15 | 18, 20, 22 | yes |
| macOS | n/a | Apple LLDB | expected (not yet tested) |
| Windows (MSVC) | not yet: nothing is emitted, so your crate builds unchanged | | |

Every GDB and LLDB version above is tested in CI on every commit, in debug, release, thin-LTO and
fat-LTO builds; VS Code was tested by hand on Linux. Details and known issues: [compatibility](docs/compatibility.md).

## Limitations

- Types defined inside function bodies aren't matched by the debugger.
- Summaries refer to fields by name; there is no expression language or format specs (yet).
- Windows/Natvis isn't supported yet.
- In plain `gdb`, standard library types (`String`, `Vec`, ...) inside your summaries show raw; use
  `rust-gdb`, which loads rustc's printers for them.

## How it works

The derive embeds a small JSON description of each type in the binary. On Linux, a section
GDB auto-loads also holds a 4 KB runtime that renders those descriptions, so GDB needs no setup
beyond trusting your build directory. LLDB can't auto-load, so `cargo debuggable setup` installs
a loader that reads the same descriptions. Details: [how it works](docs/how-it-works.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
