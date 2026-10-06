#!/usr/bin/env python3
"""Regenerate the embedded GDB runtime: debuggable/src/runtime/gdb.py -> gdb.py.zb64.

    tools/gen-runtime.py           rewrite gdb.py.zb64
    tools/gen-runtime.py --check   exit 1 if gdb.py.zb64 is stale or MINOR is out of sync (CI)

The check compares decompressed content, not bytes, because zlib output can differ
between zlib implementations.

Comment-only lines are replaced by blank lines before compressing: about 7% smaller, and
line numbers in tracebacks still match gdb.py. Docstrings and code are embedded verbatim.
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
    """gdb.py as embedded: comment-only lines blanked, everything else unchanged."""
    lines = src.split(b"\n")
    return b"\n".join(b"" if l.lstrip().startswith(b"#") else l for l in lines)


def main():
    src = embedded_form(SRC.read_bytes())
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
