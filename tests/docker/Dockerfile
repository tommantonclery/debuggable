# Pinned debugger environment for the harness (tests/harness).
#   docker build --build-arg LLDB_VERSION=22 --build-arg RUST_TOOLCHAIN=1.97.0 -t debuggable-dbg:22 tests/docker
# Use tests/docker/run.sh rather than calling this directly.
#
# Ubuntu 24.04 gives GDB 15 and LLDB 18-20 from the archive; newer LLDB comes from apt.llvm.org.
# Different LLDB majors conflict as packages, hence one image per LLDB version.
FROM ubuntu:24.04

ARG LLDB_VERSION=18
ARG RUST_TOOLCHAIN=1.97.0
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl gnupg build-essential python3 gdb binutils \
 && rm -rf /var/lib/apt/lists/*

RUN if [ "$LLDB_VERSION" -gt 20 ]; then \
      curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key | gpg --dearmor -o /usr/share/keyrings/llvm.gpg \
      && echo "deb [signed-by=/usr/share/keyrings/llvm.gpg] http://apt.llvm.org/noble/ llvm-toolchain-noble-${LLDB_VERSION} main" \
         > /etc/apt/sources.list.d/llvm.list; \
    fi \
 && apt-get update \
 && apt-get install -y --no-install-recommends lldb-${LLDB_VERSION} python3-lldb-${LLDB_VERSION} \
 && rm -rf /var/lib/apt/lists/*

# Toolchains live in /opt (world-readable) so the container can run as the host user:
# files written into the mounted repository then belong to you, not root.
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable \
 && rustup toolchain install ${RUST_TOOLCHAIN} --profile minimal \
 && chmod -R a+rX /opt/rustup /opt/cargo

# At run time cargo needs a writable home for its registry cache.
ENV CARGO_HOME=/tmp/cargo HOME=/tmp
WORKDIR /work
