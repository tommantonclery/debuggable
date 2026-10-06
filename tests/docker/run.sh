#!/usr/bin/env sh
# Run the debugger harness in a pinned Docker image.
#
#   tests/docker/run.sh 18                  # GDB 15 + LLDB 18
#   tests/docker/run.sh 22                  # LLDB 22 (CodeLLDB's version)
#   tests/docker/run.sh 20 --test debuggers # extra args go to `cargo test`
#
# New or changed snapshots are all written as *.snap.new (the run does not stop at the
# first); review them with `cargo insta review` (install with `cargo install cargo-insta`).
set -eu

v=${1:?usage: tests/docker/run.sh <lldb-major> [cargo test args...]}
shift
root=$(cd "$(dirname "$0")/../.." && pwd)
toolchain=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/tests/fixtures/rust-toolchain.toml")

debuggers="lldb-$v"
[ "$v" = 18 ] && debuggers="gdb,lldb-18"

docker build -q -t "debuggable-dbg:$v" \
  --build-arg "LLDB_VERSION=$v" --build-arg "RUST_TOOLCHAIN=$toolchain" "$root/tests/docker" >/dev/null

# SYS_PTRACE + unconfined seccomp: debuggers need ptrace, and GDB disables ASLR via personality(2).
exec docker run --rm \
  --cap-add=SYS_PTRACE --security-opt seccomp=unconfined \
  --user "$(id -u):$(id -g)" \
  -v "$root:/work" \
  -e "DEBUGGABLE_DEBUGGERS=$debuggers" \
  -e "INSTA_UPDATE=${INSTA_UPDATE:-new}" -e "INSTA_FORCE_PASS=${INSTA_FORCE_PASS:-1}" \
  "debuggable-dbg:$v" cargo test -p debuggable-harness "$@"