"""debuggable LLDB loader, schema v1. See docs/internal/schema-v1.md.

LLDB cannot auto-load anything from a binary, so `cargo debuggable setup` installs this
file and arranges for it to be imported (CodeLLDB `preRunCommands`, or ~/.lldbinit):

    command script import /path/to/debuggable_lldb.py

It reads the *same* descriptor entries the derive embeds for GDB (`.debug_gdb_scripts` on
ELF, `__DATA,__debuggable` on Mach-O) and matches types lazily through a recognizer
function, so nothing is parsed until LLDB asks about a type.

Rule for everything below: one unreadable value must never abort printing.
"""
import json

import lldb

VERSION = "1.1"           # loader version, reported by `debuggable status`
CATEGORY = "debuggable"
SECTION_NAMES = (".debug_gdb_scripts", "__debuggable")
ENTRY_PREFIX = b"\x04debuggable-v1-"
SUMMARY_MAX = 64
ITEMS_MAX = 10000

_by_module = {}           # module key -> ({exact path: desc}, {generic path: desc})
_by_type = {}             # type name -> desc or None (cache, cleared with modules)


# ---- Reading descriptors from a module ---------------------------------------------------

def _module_key(module):
    return "%s|%s" % (module.GetUUIDString(), module.GetFileSpec().fullpath)


def _sections(section_owner, count, at):
    """Yield every section, recursing into subsections (Mach-O nests __DATA,__debuggable)."""
    for i in range(count):
        s = at(i)
        yield s
        n = s.GetNumSubSections()
        if n:
            for sub in _sections(s, n, s.GetSubSectionAtIndex):
                yield sub


def _parse_entries(raw):
    """Descriptor dicts from a section's bytes (schema §3.1 framing)."""
    for entry in raw.split(b"\0"):
        if not entry.startswith(ENTRY_PREFIX):
            continue  # runtime entries, rustc's own entries, other schema majors
        a = entry.find(b"r'''")
        b = entry.find(b"'''", a + 4) if a >= 0 else -1
        if b < 0:
            continue
        try:
            # The derive emits ASCII-only JSON (schema 3.1); accept UTF-8 anyway, as GDB does.
            d = json.loads(entry[a + 4:b].decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            continue
        if d.get("v") == 1 and "path" in d:
            yield d


def _module_index(module):
    key = _module_key(module)
    idx = _by_module.get(key)
    if idx is None:
        exact, generic = {}, {}
        for sec in _sections(module, module.GetNumSections(), module.GetSectionAtIndex):
            if sec.GetName() not in SECTION_NAMES:
                continue
            err = lldb.SBError()
            raw = sec.GetSectionData().ReadRawData(err, 0, sec.GetByteSize())
            if not err.Success() or not raw:
                continue
            for d in _parse_entries(bytes(raw)):
                (generic if d.get("generic") else exact)[d["path"]] = d
        idx = _by_module[key] = (exact, generic)
    return idx


def _find(sbtype):
    name = sbtype.GetUnqualifiedType().GetName() or ""
    if name in _by_type:
        return _by_type[name]
    base = name[:name.find("<")] if name.endswith(">") else None
    modules = []
    m = sbtype.GetModule() if hasattr(sbtype, "GetModule") else None
    if m is not None and m.IsValid():
        modules.append(m)  # the defining module first: that is where its descriptor lives
    target = lldb.debugger.GetSelectedTarget() if lldb.debugger else None
    if target is not None and target.IsValid():
        modules.extend(target.GetModuleAtIndex(i) for i in range(target.GetNumModules()))
    found = None
    for mod in modules:
        exact, generic = _module_index(mod)
        found = exact.get(name) or (generic.get(base) if base else None)
        if found:
            break
    _by_type[name] = found
    return found


def recognize(sbtype, _internal_dict):
    try:
        return _find(sbtype) is not None
    except Exception:
        return False


# ---- Guarded value access ---------------------------------------------------------------

def _field(node, src_name):
    """Field by Rust source name; tuple field "0" is DWARF "__0"."""
    for cand in (src_name, "__" + src_name):
        c = node.GetChildMemberWithName(cand)
        if c.IsValid():
            return c
    return None


def _active_variant(valobj):
    """(variant value, variant name) for LLDB's clang-style encoding:
    $variants$ -> $variant$<discr> { $discr$, value }. LLDB <= 20 truncates <discr> in the
    member name to 32 bits (22.1.8 does not), so accept either form."""
    variants = valobj.GetChildMemberWithName("$variants$")
    if not variants.IsValid():
        return valobj, None
    chosen = dataful = None
    for i in range(variants.GetNumChildren()):
        v = variants.GetChildAtIndex(i)
        discr = v.GetChildMemberWithName("$discr$")
        if not discr.IsValid():
            dataful = v
            continue
        suffix = (v.GetName() or "")[len("$variant$"):]
        d = discr.GetValueAsUnsigned()
        if suffix.isdigit() and int(suffix) in (d, d & 0xFFFFFFFF):
            chosen = v
            break
    chosen = chosen or dataful
    if chosen is None:
        raise ValueError("cannot resolve active variant")
    value = chosen.GetChildMemberWithName("value")
    return value, value.GetType().GetName().rsplit("::", 1)[-1]


def _render(v):
    """One field, rendered for a summary (schema §4.2)."""
    if v is None or not v.IsValid():
        return "<unavailable>"
    s = v.GetSummary()
    if not s:
        # Values reached through children we created with CreateValueFromAddress (items)
        # never get a summary from LLDB, even on repeated calls. Re-reading the same
        # memory as a fresh value does (verified on LLDB 20).
        addr = v.GetLoadAddress()
        if addr != lldb.LLDB_INVALID_ADDRESS:
            fresh = v.CreateValueFromAddress(v.GetName() or "v", addr, v.GetType())
            if fresh.IsValid():
                s = fresh.GetSummary()
    if not s:
        s = v.GetValue()
    if not s:
        return "<unavailable>" if v.GetError().Fail() else "{...}"
    s = " ".join(s.split())
    return s if len(s) <= SUMMARY_MAX else s[:SUMMARY_MAX - 1] + "…"


def _first_ptr(v):
    for _ in range(12):
        if v.GetType().IsPointerType():
            return v
        if v.GetNumChildren() == 0:
            break
        v = v.GetChildAtIndex(0)
    raise ValueError("no pointer")


def _items(parent, node, spec):
    src = _field(node, spec["field"])
    if src is None:
        return []
    t = src.GetType()
    if (t.GetName() or "").startswith("alloc::vec::Vec<"):
        elem = t.GetTemplateArgumentType(0)
        ptr = _first_ptr(src)
        n = src.GetChildMemberWithName("len").GetValueAsUnsigned()
        if spec.get("len"):
            lf = _field(node, spec["len"])
            n = min(n, lf.GetValueAsUnsigned()) if lf is not None else n
    else:
        ptr = src if t.IsPointerType() else _first_ptr(src)  # *T or NonNull<T>
        elem = ptr.GetType().GetPointeeType()
        lf = _field(node, spec["len"]) if spec.get("len") else None
        if lf is None:
            return []
        n = lf.GetValueAsUnsigned()
    if not elem.IsValid() or elem.GetByteSize() == 0:
        return []
    base, size = ptr.GetValueAsUnsigned(), elem.GetByteSize()
    return [parent.CreateValueFromAddress("[%d]" % i, base + i * size, elem)
            for i in range(min(n, ITEMS_MAX))]


# ---- Providers --------------------------------------------------------------------------

def _pointee(valobj):
    """The raw value to format. LLDB applies a type's formatters through pointers and
    references to it (e.g. a `&Token` field), so dereference those first."""
    v = valobj.GetNonSyntheticValue()
    t = v.GetType()
    if t.IsPointerType() or t.IsReferenceType():
        v = v.Dereference().GetNonSyntheticValue()
    return v


def _node(raw, d):
    """(node whose fields we show, variant name or None, variant/struct descriptor)."""
    if d.get("kind") == "enum":
        node, variant = _active_variant(raw)
        return node.GetNonSyntheticValue(), variant, d.get("variants", {}).get(variant) or {}
    return raw, None, d

def summary(valobj, _internal_dict):
    try:
        raw = _pointee(valobj)
        d = _find(raw.GetType())
        if d is None:
            return ""
        node, variant, vd = _node(raw, d)
        parts = vd.get("summary")
        if parts is None:
            if variant:
                return variant
            # No summary: LLDB shows the children; if there are none, say so explicitly.
            # Return "" (not None) for "no summary": LLDB 20 prints a returned None as "None".
            return "{}" if valobj.GetNumChildren() == 0 else ""
        return "".join(t if k == "lit" else _render(_field(node, t)) for k, t in parts)
    except Exception as e:  # last resort: never break the variables view
        return "<debuggable: %s>" % e


class Synth:
    def __init__(self, valobj, _internal_dict):
        self.valobj = valobj
        self.kids = []

    def update(self):
        self.kids = []
        try:
            raw = _pointee(self.valobj)
            d = _find(raw.GetType())
            if d is None:
                return False
            # Iterate *raw* children: rustc's own synthetic providers can fail (e.g. tuple
            # structs on LLDB 18), which would leave us with no children at all.
            node, _variant, vd = _node(raw, d)
            hide = set(vd.get("hide", ()))
            rename = vd.get("rename", {})
            items = vd.get("items")
            if items:
                hide.add(items["field"])
            for i in range(node.GetNumChildren()):
                c = node.GetChildAtIndex(i)
                n = c.GetName() or ""
                if n.startswith("$"):
                    continue
                src = n[2:] if n.startswith("__") and n[2:].isdigit() else n
                if src in hide:
                    continue
                label = rename.get(src, src)
                self.kids.append(c.Clone(label) if label != n else c)
            if items:
                self.kids.extend(_items(self.valobj, node, items))
        except Exception:
            pass  # keep whatever children were collected
        return False

    def num_children(self, max_children=None):
        return len(self.kids)

    def get_child_at_index(self, i):
        return self.kids[i] if 0 <= i < len(self.kids) else None

    def get_child_index(self, name):
        for i, k in enumerate(self.kids):
            if k.GetName() == name:
                return i
        return -1

    def has_children(self):
        return bool(self.kids)


# ---- Category priority ------------------------------------------------------------------
# LLDB uses the first enabled category with a match, and rustc's "Rust" category has
# catch-all regexes. If Rust is enabled after us (rust-lldb, or CodeLLDB with
# initCommands), it shadows our summaries. Re-enabling moves our category to the front.

def _priority_order(debugger):
    """Category names in lookup-priority order. The SB API's GetCategoryAtIndex order is
    *not* priority order, so read what LLDB itself prints."""
    res = lldb.SBCommandReturnObject()
    debugger.GetCommandInterpreter().HandleCommand("type category list", res)
    names = []
    for line in (res.GetOutput() or "").splitlines():
        if line.startswith("Category: "):
            names.append(line[len("Category: "):].split(" (", 1)[0])
    return names


def _ensure_first(debugger):
    names = _priority_order(debugger)
    if CATEGORY in names and "Rust" in names and names.index("Rust") < names.index(CATEGORY):
        debugger.HandleCommand("type category disable " + CATEGORY)
        debugger.HandleCommand("type category enable " + CATEGORY)


class OrderHook:
    """Stop hook, run on every stop (cheap): re-checks category order, catching formatters
    loaded after this script, and drops cached descriptors when the set of loaded modules
    changes (rebuilt binary, newly loaded shared library)."""

    def __init__(self, target, _extra_args, _internal_dict):
        self.debugger = target.GetDebugger()
        self.modules = None

    def handle_stop(self, exe_ctx, _stream):
        try:
            _ensure_first(self.debugger)
            t = exe_ctx.GetTarget()
            sig = tuple(_module_key(t.GetModuleAtIndex(i)) for i in range(t.GetNumModules()))
            if sig != self.modules:
                self.modules = sig
                _by_module.clear()
                _by_type.clear()
        except Exception:
            pass
        return True  # stay stopped


# ---- `debuggable status` ----------------------------------------------------------------

def _status(debugger, _command, result, _internal_dict):
    names = _priority_order(debugger)
    out = ["debuggable LLDB loader %s" % VERSION]
    if "Rust" in names and CATEGORY in names:
        ok = names.index(CATEGORY) < names.index("Rust")
        out.append("category order: %s" % ("ok (debuggable before Rust)" if ok else
                                           "WRONG: Rust shadows debuggable summaries"))
    else:
        out.append("category order: Rust formatters not loaded")
    target = debugger.GetSelectedTarget()
    if target.IsValid():
        for i in range(target.GetNumModules()):
            m = target.GetModuleAtIndex(i)
            exact, generic = _module_index(m)
            if exact or generic:
                out.append("%5d descriptors  %s" % (len(exact) + len(generic), m.GetFileSpec().fullpath))
    result.AppendMessage("\n".join(out))


def __lldb_init_module(debugger, _internal_dict):
    m = __name__
    debugger.HandleCommand("type summary add -w %s -e --recognizer-function %s.recognize -F %s.summary"
                           % (CATEGORY, m, m))
    debugger.HandleCommand("type synthetic add -w %s --recognizer-function %s.recognize -l %s.Synth"
                           % (CATEGORY, m, m))
    debugger.HandleCommand("type category enable " + CATEGORY)
    # Stop hooks added before a target exists go on LLDB's dummy target and are copied
    # to every target created later, so this also covers ~/.lldbinit imports.
    debugger.HandleCommand("target stop-hook add -P %s.OrderHook" % m)
    debugger.HandleCommand("command script add -f %s._status debuggable-status" % m)
