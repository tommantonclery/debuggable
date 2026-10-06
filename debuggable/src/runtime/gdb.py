"""debuggable GDB runtime, schema v1. See docs/internal/schema-v1.md.

Embedded (zlib + base64) by the `debuggable` facade as one `.debug_gdb_scripts` entry and
executed by GDB's auto-loader in a private namespace. Descriptor entries (one per derived
type) queue their JSON in `gdb._debuggable_q` and call `gdb._debuggable_flush` if it exists,
so load order between entries does not matter.

Rule for everything below: one unreadable value must never abort printing. Every access
to target memory goes through a guard, and both printer methods have a last-resort catch.
"""
import json

import gdb

MINOR = 3           # bump on any change; must match the entry name in __private.rs
SUMMARY_MAX = 64    # characters per rendered field in a summary
ITEMS_MAX = 10000   # hard cap on items children

_SCALARS = (gdb.TYPE_CODE_INT, gdb.TYPE_CODE_BOOL, gdb.TYPE_CODE_CHAR, gdb.TYPE_CODE_FLT)


# ---- Inter-runtime protocol (frozen for all of v1, see schema §3.3) ---------------------
# gdb._debuggable_q      list of (objfile, json_str), appended by descriptor entries
# gdb._debuggable_flush  callable, drains the queue
# gdb._debuggable_state  {"descs": {objfile_key: [descriptor dict, ...]}}
# gdb._debuggable_rt     the active runtime; has .MINOR and .lookup(objfile_key, value)
# Registered lookups only ever call gdb._debuggable_rt.lookup(key, value), so a newer
# runtime replacing an older one takes over printers the older one registered.

class _Lookup:
    name = "debuggable"

    def __init__(self, key):
        self.key = key
        self.enabled = True
        self.subprinters = None

    def __call__(self, val):
        rt = getattr(gdb, "_debuggable_rt", None)
        return rt.lookup(self.key, val) if rt is not None else None


class _Runtime:
    MINOR = MINOR

    def __init__(self):
        st = getattr(gdb, "_debuggable_state", None)
        if st is None:
            st = gdb._debuggable_state = {"descs": {}}
        self.state = st
        self._index = {}

    def flush(self):
        q = gdb.__dict__.setdefault("_debuggable_q", [])
        while q:
            objfile, raw = q.pop(0)
            try:
                d = json.loads(raw)
            except ValueError:
                continue
            if d.get("v") != 1 or "path" not in d:
                continue  # another schema major's runtime handles it
            key = objfile.filename if objfile is not None else ""
            self.state["descs"].setdefault(key, []).append(d)
            self._register(objfile, key)

    @staticmethod
    def _register(objfile, key):
        target = objfile.pretty_printers if objfile is not None else gdb.pretty_printers
        if not any(getattr(p, "name", None) == "debuggable" for p in target):
            target.insert(0, _Lookup(key))

    def _idx(self, key):
        descs = self.state["descs"].get(key, ())
        cached = self._index.get(key)
        if cached is None or cached[0] != len(descs):
            exact, generic = {}, {}
            for d in descs:
                (generic if d.get("generic") else exact)[d["path"]] = d
            cached = self._index[key] = (len(descs), exact, generic)
        return cached[1], cached[2]

    def lookup(self, key, val):
        try:
            t = val.type.strip_typedefs()
            name = t.name or t.tag
            if not name:
                return None
            exact, generic = self._idx(key)
            d = exact.get(name)
            if d is None and name.endswith(">"):
                d = generic.get(name[:name.find("<")])
            return _Printer(val, d) if d is not None else None
        except Exception:
            return None  # fall back to GDB's default rendering


# ---- Guarded value access ---------------------------------------------------------------

def _fetched(v):
    """`v` with its contents read, or None if target memory is unreadable / optimized out."""
    try:
        v.fetch_lazy()
        return v
    except gdb.error:
        return None


def _field(node, src_name):
    """Field by Rust source name; tuple field "0" is DWARF "__0". None if absent."""
    for f in node.type.fields():
        if f.name == src_name or f.name == "__" + src_name:
            return node[f]
    return None


def _first_ptr(v):
    """First pointer reached through first fields (Vec's buffer, NonNull's pointer)
    without naming std internals."""
    for _ in range(12):
        t = v.type.strip_typedefs()
        if t.code == gdb.TYPE_CODE_PTR:
            return v
        fields = t.fields() if t.code == gdb.TYPE_CODE_STRUCT else ()
        if not fields:
            break
        v = v[fields[0]]
    raise ValueError("no pointer")


def _active_variant(val):
    """(variant value, variant name). GDB exposes only the active variant as a named field,
    but values reached through arrays arrive unresolved (all variants listed), so re-read
    them through their address."""
    named = [f for f in val.type.fields() if f.name]
    if len(named) > 1 and val.address is not None:
        val = val.address.dereference()
        named = [f for f in val.type.fields() if f.name]
    if len(named) != 1:
        raise gdb.error("cannot resolve active variant")
    return val[named[0]], named[0].name


def _is_lazy_string(r):
    # gdb.LazyString is not exposed as a module attribute in every GDB (absent in 15.1),
    # so detect it by shape.
    return (r is not None and not isinstance(r, (str, gdb.Value))
            and hasattr(r, "address") and hasattr(r, "length") and hasattr(r, "encoding"))


def _lazy_string_text(r):
    """A lazy string as a quoted, escaped literal (reads at most 256 units)."""
    try:
        if r.length >= 0:
            raw = gdb.selected_inferior().read_memory(r.address, min(r.length, 256)).tobytes()
            s = raw.decode(r.encoding or "utf-8", "replace")
        else:
            s = r.value().string(length=256)
    except (gdb.error, LookupError):
        return "<unavailable>"
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _render(v):
    """One field, rendered for a summary (schema §4.2)."""
    if v is None:
        return "<unavailable>"
    if _fetched(v) is None:
        return "<optimized out>"
    try:
        if v.type.strip_typedefs().code in _SCALARS:
            s = str(v)
        else:
            s = None
            pp = gdb.default_visualizer(v)  # ours or anyone's (e.g. rustc's std printers)
            if pp is not None and hasattr(pp, "to_string"):
                r = pp.to_string()
                if _is_lazy_string(r):  # e.g. rustc's String/&str printers
                    r = _lazy_string_text(r)
                if isinstance(r, gdb.Value):
                    r = r.format_string()
                if r is not None:
                    s = str(r)
            if s is None:
                s = _without_type_path(v, v.format_string(pretty_structs=False, max_elements=8, max_depth=1))
    except gdb.error:
        return "<unavailable>"
    s = " ".join(s.split())
    return s if len(s) <= SUMMARY_MAX else s[:SUMMARY_MAX - 1] + "…"


def _for_host(s):
    """GDB raises UnicodeEncodeError when a printer returns characters the host charset
    can't represent (e.g. LC_ALL=C in CI and containers). Escape those Rust-style."""
    try:
        enc = gdb.host_charset()  # GDB >= 12
        s.encode(enc)
        return s
    except UnicodeEncodeError:
        out = []
        for c in s:
            try:
                c.encode(enc)
                out.append(c)
            except UnicodeEncodeError:
                out.append("\\u{%x}" % ord(c))
        return "".join(out)
    except (AttributeError, LookupError):
        return s  # older GDB or unknown charset name: leave as is


def _without_type_path(v, s):
    """GDB's own Rust printing starts with the full type path:
    `core::option::Option<alloc::string::String>::Some("a")`, `my::Point {x: 1}`.
    In a one-line summary that is noise (LLDB shows `Some("a")`), so drop it."""
    name = v.type.strip_typedefs().name
    if name and s.startswith(name):
        rest = s[len(name):]
        if rest.startswith("::"):
            return rest[2:]
        if rest.startswith(" {"):
            return rest[1:]
    return s


def _int_field(node, name):
    v = _field(node, name)
    if v is None or _fetched(v) is None:
        raise ValueError(name)
    return int(v)


def _items(node, spec):
    """Children `[0]`, `[1]`, ... for an `items` field (schema §4)."""
    src = _field(node, spec["field"])
    if src is None:
        yield "[..]", "<unavailable>"
        return
    try:
        t = src.type.strip_typedefs()
        if (t.name or "").startswith("alloc::vec::Vec<"):
            elem = t.template_argument(0)
            ptr = _first_ptr(src)
            n = int(src["len"])
            if spec.get("len"):
                n = min(n, _int_field(node, spec["len"]))
        else:
            ptr = src if t.code == gdb.TYPE_CODE_PTR else _first_ptr(src)  # *T or NonNull<T>
            elem = ptr.type.strip_typedefs().target()
            if not spec.get("len"):
                yield "[..]", "<items: len required>"
                return
            n = _int_field(node, spec["len"])
        ptr = ptr.cast(elem.pointer())
    except ValueError:
        yield "[..]", "<unsupported items source>"
        return
    except (gdb.error, RuntimeError):
        yield "[..]", "<optimized out>"
        return
    for i in range(max(0, min(n, ITEMS_MAX))):
        try:
            el = (ptr + i).dereference()
            el.fetch_lazy()
        except gdb.error:
            el = "<unavailable>"
        yield "[%d]" % i, el


class _Printer:
    def __init__(self, val, desc):
        self.node, self.variant, self.vd = val, None, desc
        if desc.get("kind") == "enum":
            self.node, self.variant = _active_variant(val)
            self.vd = desc.get("variants", {}).get(self.variant) or {}

    def to_string(self):
        try:
            parts = self.vd.get("summary")
            if parts is None:
                if self.variant is not None:
                    return self.variant
                # No summary: let GDB show the children, but never an empty " =".
                return None if any(True for _ in self._children()) else "{}"
            return _for_host("".join(
                text if kind == "lit" else _render(_field(self.node, text)) for kind, text in parts
            ))
        except Exception as e:  # last resort: never abort the user's print
            return "<debuggable: %s>" % e

    def children(self):
        try:
            yield from self._children()
        except Exception as e:
            yield "<debuggable>", str(e)

    def _children(self):
        node = self.node
        if node.type.strip_typedefs().code != gdb.TYPE_CODE_STRUCT:
            return
        hide = set(self.vd.get("hide", ()))
        rename = self.vd.get("rename", {})
        items = self.vd.get("items")
        if items:
            hide.add(items["field"])
        for f in node.type.fields():
            if not f.name or f.artificial:
                continue
            src = f.name[2:] if f.name.startswith("__") and f.name[2:].isdigit() else f.name
            if src in hide:
                continue
            v = _fetched(node[f])
            yield rename.get(src, src), (v if v is not None else "<optimized out>")
        if items:
            yield from _items(node, items)


def _install():
    cur = getattr(gdb, "_debuggable_rt", None)
    if cur is not None and getattr(cur, "MINOR", -1) >= MINOR:
        return cur  # an equal or newer runtime is already active
    rt = _Runtime()
    gdb._debuggable_rt = rt
    gdb._debuggable_flush = rt.flush
    return rt


_install().flush()
