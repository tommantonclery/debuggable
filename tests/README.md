# Tests

## Debugger harness (`tests/harness`)

Builds the programs in `tests/fixtures`, runs each one under real debuggers in batch mode,
and snapshot-tests what they print with [insta](https://insta.rs).

- **Fixtures** stop in `debuggable_fixture_stop(f: &Fixture)`. The harness prints each field
  of `f`. Until the derive exists (Phase 4) they use hand-written `__entry!` calls.
- **Matrix:** 4 fixtures × every available debugger × 4 profiles (`dev`, `release`,
  `release-thin`, `release-fat`). Snapshots are per debugger major (`gdb15`, `lldb18`, …),
  because debuggers and rustc's own formatters render std types differently across versions.
- **Hard failures**, independent of snapshots: the breakpoint not being hit, a field printing
  nothing, or any error from our runtime or loader (`<debuggable: …>`, tracebacks in our code,
  GDB's `Python Exception`).
- **Structural tests** (`tests/structure.rs`): every entry survives linking in every profile
  and `strip`, `--cfg debuggable_disable` emits nothing and causes no warnings, and the
  embedded GDB runtime is fresh.

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

### Changing the pinned toolchain

Edit `tests/fixtures/rust-toolchain.toml`. Docker and CI read the version from there. Expect
std-type snapshots to change (rustc ships the std formatters): review them all.
