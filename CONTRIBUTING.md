# Contributing

## Layout

Three crates, released together. `debuggable` is the facade users depend on, `debuggable-derive`
is the proc macro behind it, `cargo-debuggable` is the `setup` and `doctor` binary. The facade
exists largely to own the `macro_rules!` that writes the entry static: `#[link_section]` has to
live there rather than in the derive's output, so that it carries the facade's edition and does
not trip a user's `unsafe_code` lint (schema-v1 §2.2).

Rendering lives in two places that cannot share code — `debuggable/src/runtime/gdb.py`, embedded
in every Linux binary, and `cargo-debuggable/assets/debuggable_lldb.py`, installed on the machine
by `setup`. They implement the same semantics against two incompatible debugger APIs, and
[`docs/internal/schema-v1.md`](docs/internal/schema-v1.md) is what keeps them agreeing. The derive
itself never emits debugger code; descriptors are data.

`tests/fixtures` is a separate workspace, excluded from the main one, because the harness builds
it with the toolchain pinned in `tests/fixtures/rust-toolchain.toml` — rustc ships the std-type
formatters whose output ends up in the snapshots.

## Running the tests

```sh
cargo test -p debuggable-derive -p cargo-debuggable   # the derive, and the CLI end to end
cargo test -p debuggable --doc                        # the crate-doc examples
```

Those two cover everything except rendering. Anything that changes what a type looks like in a
debugger — either runtime, the schema, a new attribute — has to go through the snapshot harness,
and [`tests/README.md`](tests/README.md) covers that and the rest of the test suite.

Before pushing, what CI fails you on first:

```sh
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo build --workspace --exclude debuggable-harness
cargo clippy --workspace --exclude debuggable-harness --all-targets -- -D warnings
```

The MSRV is Rust 1.75, checked in CI against the three published crates only; the harness's test
dependencies may need something newer.

## Formatting

`rustfmt.toml` sets 125 columns rather than the default 100, because that is closest to how the
code was already written. CI runs `cargo fmt --all --check`, so run `cargo fmt --all` before you
push.

Three blocks are laid out as tables by hand and carry `#[rustfmt::skip]` to stay that way: the test
module in `debuggable-derive/src/lib.rs` (one case per line, input beside expected output), the
field lists in `tests/harness/tests/debuggers.rs`, and the DWARF tag list in
`cargo-debuggable/src/doctor.rs`. Add rows
to those matching the row above. `tests/fixtures` is outside the workspace, so `cargo fmt` never
reaches it — match the file you are editing there.

## Changing the descriptor schema

[`docs/internal/schema-v1.md`](docs/internal/schema-v1.md) is the contract between the derive, the
facade, the GDB runtime and the LLDB loader. It is internal — no stability promise to users — but
all four have to agree, so a change there touches more files than you would expect.

The rule from §7: if an older runtime would *ignore* your addition, it is additive — add a new
optional key and bump the GDB runtime's minor. If an older runtime would **misrender** the value,
it needs schema v2, a new entry prefix and both runtimes coexisting. `slots` and `alternatives` are
both separate keys rather than extensions of `items` for exactly this reason.

An additive change means, in order:

1. Teach the derive to emit the new key, with validation and an error message for each way it can
   be wrong, plus unit tests asserting the JSON character-for-character, and a `tests/ui` case if
   it brings a span worth pinning.
2. Implement it in **both** runtimes — `debuggable/src/runtime/gdb.py` and
   `cargo-debuggable/assets/debuggable_lldb.py`. They cannot share code (one is embedded in the
   binary, the other installed on the machine), so the schema document is what keeps them honest.
3. Bump `MINOR` in `gdb.py` and the matching entry name in `debuggable/src/__private.rs`, then run
   `python3 tools/gen-runtime.py` to regenerate `gdb.py.zb64`.
4. Bump `VERSION` in `debuggable_lldb.py`.
5. Record the key in schema-v1.md §4, annotated with the runtime minor and loader version that
   introduced it, so a reader can tell what an older runtime will do with it.
6. Add a fixture case and review the new snapshots on every debugger.
7. Write a design note in `docs/design/` if the change needed a decision rather than just code.

## Releasing

All three crates share one version and are released together.

Three of the six steps below are checked for you: `python3 tools/gen-runtime.py --check` covers 2
and 3, and cargo refuses to build at all if 6 is stale, because the local path dependency has to
satisfy the pin. Nothing checks 1, 4 or 5, so walk the list:

| | Bump | Where |
|---|---|---|
| 1 | Workspace version | `version` in the root `Cargo.toml` |
| 2 | GDB runtime minor | `MINOR` in `debuggable/src/runtime/gdb.py`, if the runtime changed |
| 3 | Runtime entry name | `debuggable-runtime-gdb-v1.<minor>` in `debuggable/src/__private.rs`, to match, then regenerate `gdb.py.zb64` |
| 4 | LLDB loader version | `VERSION` in `cargo-debuggable/assets/debuggable_lldb.py`, if the loader changed |
| 5 | Schema annotations | the "runtime minor N / loader N" notes in schema-v1.md §4 |
| 6 | The exact pin | `debuggable-derive = { version = "=X.Y.Z" }` in `debuggable/Cargo.toml` |

Then `CHANGELOG.md`: a section per crate, and say so when the loader changed, because users have to
re-run `cargo debuggable setup` to pick it up.

Publish `debuggable-derive` first — the `=` pin in step 6 cannot resolve until it is on crates.io —
then `debuggable`, then `cargo-debuggable`:

```sh
cargo publish -p debuggable-derive
cargo publish -p debuggable
cargo publish -p cargo-debuggable
```

The pin is what guarantees a binary never mixes a derive with a facade that doesn't understand its
output. Publishing in that order matters for the same reason: `cargo publish -p debuggable` builds
the packaged crate without the path dependency, so the pinned derive version has to be on crates.io
already.
