# Tests

## Debugger harness (`tests/harness`)

Builds the programs in `tests/fixtures`, runs each one under real debuggers in batch mode,
and snapshot-tests what they print with [insta](https://insta.rs).

- **Fixtures** stop in `debuggable_fixture_stop(f: &Fixture)`. The harness prints each field
  of `f`. Every fixture defines that function itself instead of sharing one, so nothing but the
  fixture's own types reaches its binary — which is the point of the `deps` fixture. The empty
  `asm!` in it keeps `f` live and its pointee in memory without a call: `black_box` would be
  inlined in release builds, and the breakpoint would then land in its inlined frame, where `f`
  is not in scope.
- **Matrix:** 7 fixtures (`structs`, `enums`, `items`, `text`, `slots`, `alternatives` and
  `deps`) × every available debugger × 4 profiles (`dev`, `release`, `release-thin`,
  `release-fat`). The committed snapshots cover five debugger majors, so the directory holds
  7 × 5 × 4 = 140 of them. Snapshots are per debugger major (`gdb15`, `lldb18`, …), because
  debuggers and rustc's own formatters render std types differently across versions.
- **Hard failures**, independent of snapshots: the breakpoint not being hit, a field printing
  nothing, or any error from our runtime or loader (`<debuggable: …>`, tracebacks in our code,
  GDB's `Python Exception`).
- **Structural tests** (`tests/structure.rs`): every entry survives linking in every profile and
  under both `strip` and `strip --strip-debug`, `--cfg debuggable_disable` emits nothing and causes
  no warnings, the entry sizes stay inside the budget in schema-v1 §8, and the embedded GDB
  runtime is fresh.

### Running locally

```bash
rustup toolchain install 1.97.0 --profile minimal     # the pinned fixture toolchain
cargo test -p debuggable-harness                       # uses whichever debuggers you have
```

Without any debugger installed, the snapshot tests are skipped with a message.

- `DEBUGGABLE_DEBUGGERS=gdb,lldb-20` selects exact debuggers (missing ones are an error).
- `DEBUGGABLE_PROFILES=dev` limits profiles.
- `DEBUGGABLE_FIXTURE_TOOLCHAIN=stable` overrides the pinned fixture toolchain (snapshots may then differ).

**Pinned versions in Docker**, matching CI:

```bash
tests/docker/run.sh 18    # GDB 15 + LLDB 18
tests/docker/run.sh 20    # LLDB 20
tests/docker/run.sh 22    # LLDB 22, the version CodeLLDB bundles
```

### Reviewing snapshot changes

```bash
cargo install cargo-insta
cargo insta review
```

**Read every snapshot you accept.** A snapshot only proves the output didn't change, not that
it's right. CI uploads new or changed snapshots as an artifact (`snapshots-*`) when a debugger
job fails: download it, put the `.snap.new` files in `tests/harness/tests/snapshots/`, then
review and commit.

## Compile-fail tests (`debuggable/tests/ui`)

What a user sees for an invalid `#[debuggable(...)]` attribute. rustc's diagnostic layout is part
of the expected output, so these run on the pinned toolchain too:

```bash
cargo +1.97.0 test -p debuggable --test ui
```

The messages themselves are covered by `debuggable-derive`'s unit tests; these pin the spans, so
there is one case per kind of span rather than one per message.

## Guards

```bash
python3 tools/gen-runtime.py --check    # gdb.py.zb64 is current, MINOR matches, and the
                                        #   embedded form of gdb.py is valid Python
python3 tools/bench-compile-time.py     # the derive still costs ~1 ms per type on a rebuild
```

Both run in CI. The first is the one to remember after editing `gdb.py`; `CONTRIBUTING.md` has the
rest of what a runtime change involves.

## Changing the pinned toolchain

Edit `tests/fixtures/rust-toolchain.toml`; Docker and CI read the version from there. Two sets of
expected output move with it, and both need reviewing: the snapshots, because rustc ships the
std-type formatters that appear in them, and the `.stderr` files above, because rustc owns the
diagnostic layout.
