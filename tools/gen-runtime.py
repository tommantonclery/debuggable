#!/usr/bin/env python3
"""Regenerate the embedded GDB runtime: debuggable/src/runtime/gdb.py -> gdb.py.zb64.

    tools/gen-runtime.py           rewrite gdb.py.zb64
    tools/gen-runtime.py --check   exit 1 if gdb.py.zb64 is stale or MINOR is out of sync (CI)

The check compares decompressed content, not bytes, because zlib output can differ
between zlib implementations.

Before compressing, comment-only lines are blanked and docstrings reduced to `""`: smaller,
and line numbers in tracebacks still match gdb.py. Code is embedded verbatim.
"""
import base64
import pathlib
import re
import sys
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "debuggable/src/runtime/gdb.py"
OUT = ROOT / "debuggable/src/runtime/gdb.py.zb64"
RUST = ROOT / "debuggable/src/__private.rs"


def embedded_form(src):
    """gdb.py as embedded: comment-only lines blanked and docstrings reduced to `""`.

    Line numbers are preserved, so tracebacks still point at the right line of gdb.py.

    Only a triple-quoted string that starts its own line survives this correctly, which is true
    of docstrings. Anything else is mangled: a multi-line string whose closing quotes sit on their
    own line comes out unterminated, and a string literal alone on a line is replaced by `""`.
    `main` therefore compiles the result -- comparing it with itself, as `--check` does, cannot
    catch either, and GDB would reject the whole script.
    """
    out = []
    in_doc = False
    for line in src.split(b"\n"):
        stripped = line.lstrip()
        if in_doc:
            out.append(b"")
            if b'"""' in line:
                in_doc = False
        elif stripped.startswith(b'"""'):
            indent = line[: len(line) - len(stripped)]
            out.append(indent + b'""')
            in_doc = stripped.count(b'"""') == 1  # opening only: the docstring continues
        elif stripped.startswith(b"#"):
            out.append(b"")
        else:
            out.append(line)
    return b"\n".join(out)


def main():
    src = embedded_form(SRC.read_bytes())
    try:
        compile(src, str(SRC), "exec")
    except SyntaxError as e:
        sys.exit("error: %s is not valid Python once embedded (%s, line %s); see embedded_form"
                 % (SRC.relative_to(ROOT), e.msg, e.lineno))
    minor = re.search(rb"^MINOR = (\d+)", src, re.M).group(1).decode()
    name = "debuggable-runtime-gdb-v1.%s" % minor
    if name not in RUST.read_text():
        sys.exit("error: gdb.py has MINOR = %s but __private.rs does not embed '%s'" % (minor, name))

    if "--check" in sys.argv:
        try:
            current = zlib.decompress(base64.b64decode(OUT.read_bytes()))
        except (OSError, ValueError, zlib.error):
            current = None
        if current != src:
            sys.exit("error: %s is stale; run tools/gen-runtime.py" % OUT.relative_to(ROOT))
        print("ok: embedded runtime is up to date (%s)" % name)
        return

    OUT.write_bytes(base64.b64encode(zlib.compress(src, 9)))  # no trailing newline: embedded in a string
    print("wrote %s: %d bytes (from %d) for %s" % (OUT.relative_to(ROOT), OUT.stat().st_size, len(src), name))


if __name__ == "__main__":
    main()
