#!/usr/bin/env python3
"""Compile-time guard for #[derive(Debuggable)].

Generates two crates with the same N types, one plain and one deriving `Debuggable` with
attributes on every type, then times *rebuilds* of each (derive output is not cached by
incremental compilation, so this is the cost users pay on every edit). Builds alternate
between the two crates so drift on a shared machine affects both equally.

    tools/bench-compile-time.py                 # 1000 types, 7 rebuilds each
    tools/bench-compile-time.py --types 300 --runs 5 --max-ms-per-type 0

Fails (exit 1) if the derive's cost per type exceeds --max-ms-per-type. The cost is
(median rebuild with derives - median rebuild without) / types: the difference is large
compared with either build's noise, so it is far steadier than either time alone.
"""
import argparse
import os
import pathlib
import statistics
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "target" / "compile-time-bench"

# Measured 2026-10-06, 1000 types, 2-core Linux VM: 0.79-0.84 ms per type over four runs.
# About 3x headroom for slower CI machines; still catches a derive that emits much more
# code or does work that grows with the number of types.
DEFAULT_MAX_MS_PER_TYPE = 2.5


def generate(n, derive):
    d = derive
    out = ["#![allow(dead_code)]", "use std::collections::HashMap;", "use std::marker::PhantomData;"]
    if d:
        out.append("use debuggable::Debuggable;")

    def attr(s):
        return s if d else ""

    for i in range(n):
        k = i % 3
        if k == 0:
            out.append(attr('#[derive(Debuggable)]\n#[debuggable(summary = "#{id} {name}")]\n') + f"""pub struct Rec{i} {{
    pub id: u64,
    pub name: String,
    {attr('#[debuggable(hide)]')} pub cache: HashMap<String, Vec<u8>>,
    {attr('#[debuggable(rename = "callback")]')} pub cb: Option<Box<dyn Fn(u8) -> u8>>,
    pub tags: Vec<(u16, &'static str)>,
}}""")
        elif k == 1:
            out.append(attr("#[derive(Debuggable)]\n") + f"""pub enum Ev{i} {{
    Idle,
    {attr('#[debuggable(summary = "Move({0}, {1})")]')}
    Move(u64, String),
    {attr('#[debuggable(summary = "Resize to {w}x{h}")]')}
    Resize {{ w: u32, h: u32, {attr('#[debuggable(hide)]')} raw: Vec<u8> }},
}}""")
        else:
            out.append(attr('#[derive(Debuggable)]\n#[debuggable(summary = "{len} items")]\n') + f"""pub struct Pool{i}<'a, T: Clone + 'a, const N: usize> where T: Default {{
    {attr('#[debuggable(items, len = "len")]')} slots: Vec<T>,
    len: usize,
    _p: PhantomData<&'a ()>,
    buf: [T; N],
}}""")
    return "\n\n".join(out) + "\n"


def write_crate(name, n, derive):
    path = OUT / name
    (path / "src").mkdir(parents=True, exist_ok=True)
    (path / "Cargo.toml").write_text(
        f'[package]\nname = "{name}"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n'
        f'[dependencies]\ndebuggable = {{ path = "{(ROOT / "debuggable").as_posix()}" }}\n\n[workspace]\n'
    )
    (path / "src" / "lib.rs").write_text(generate(n, derive))
    return path


def build(path):
    env = {k: v for k, v in os.environ.items() if k not in ("RUSTFLAGS", "CARGO_TARGET_DIR")}
    start = time.perf_counter()
    r = subprocess.run(["cargo", "build", "-q"], cwd=path, env=env, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if r.returncode != 0:
        sys.exit(f"build failed in {path}:\n{r.stderr}")
    return elapsed


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--types", type=int, default=1000)
    ap.add_argument("--runs", type=int, default=7)
    ap.add_argument("--max-ms-per-type", type=float, default=DEFAULT_MAX_MS_PER_TYPE, help="0 disables the check")
    a = ap.parse_args()

    base = write_crate("ct-base", a.types, derive=False)
    derived = write_crate("ct-derive", a.types, derive=True)
    print(f"warming up ({a.types} types per crate)...", flush=True)
    build(base)
    build(derived)

    times = {"base": [], "derive": []}
    for _ in range(a.runs):
        for key, path in (("base", base), ("derive", derived)):
            (path / "src" / "lib.rs").touch()
            times[key].append(build(path))

    b = statistics.median(times["base"])
    d = statistics.median(times["derive"])
    ratio = d / b
    per_type_ms = (d - b) / a.types * 1000
    print(f"rebuild without derives: {b:.2f}s (runs: {', '.join(f'{t:.2f}' for t in times['base'])})")
    print(f"rebuild with derives:    {d:.2f}s (runs: {', '.join(f'{t:.2f}' for t in times['derive'])})")
    limit = a.max_ms_per_type
    print(f"derive cost: {per_type_ms:.2f} ms per type (limit {limit or 'none'}); rebuild ratio {ratio:.2f}")
    if limit and per_type_ms > limit:
        print(f"error: the derive costs {per_type_ms:.2f} ms per type on rebuilds (limit {limit}).")
        print("If this is intended, measure locally and raise DEFAULT_MAX_MS_PER_TYPE with a note.")
        sys.exit(1)


if __name__ == "__main__":
    main()
