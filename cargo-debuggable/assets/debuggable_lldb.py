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

VERSION = "1.5"           # loader version, reported by `debuggable status`
CATEGORY = "debuggable"
SECTION_NAMES = (".debug_gdb_scripts", "__debuggable")
ENTRY_PREFIX = b"\x04debuggable-v1-"
SUMMARY_MAX = 64
ITEMS_MAX = 10000
TEXT_MAX = 1024           # bytes read for a text field

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


def _variant_name(type_name):
    """`a::Msg<u32>::Data<u32>` -> `Data`. LLDB names variant types with the enum's generic
    arguments appended, which may themselves contain `::`."""
    name = type_name or ""
    if name.endswith(">"):
        depth = 0
        for i in range(len(name) - 1, -1, -1):
            depth += {">": 1, "<": -1}.get(name[i], 0)
            if depth == 0:
                name = name[:i]
                break
    return name.rsplit("::", 1)[-1]


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
    return value, _variant_name(value.GetType().GetName())


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
    return _clip(" ".join(s.split()))


def _clip(s):
    return s if len(s) <= SUMMARY_MAX else s[:SUMMARY_MAX - 1] + "…"


def _first_ptr(v):
    for _ in range(12):
        if v.GetType().IsPointerType():
            return v
        if v.GetNumChildren() == 0:
            break
        v = v.GetChildAtIndex(0)
    raise ValueError("no pointer")


# std's repr(transparent) element wrappers: inline buffers store `[MaybeUninit<T>; N]`.
_TRANSPARENT = ("MaybeUninit", "ManuallyDrop", "MaybeDangling")


def _unwrap_transparent(elem):
    """`MaybeUninit<T>` (and the like) -> `T`. Same size and address, by repr(transparent)."""
    for _ in range(4):
        name = (elem.GetUnqualifiedType().GetName() or "").split("<", 1)[0]
        if not (name.startswith("core::mem::") and name.rsplit("::", 1)[-1] in _TRANSPARENT):
            break
        inner = elem.GetTemplateArgumentType(0)
        if not inner.IsValid() or inner.GetByteSize() != elem.GetByteSize():
            break  # not transparent after all: keep the wrapper
        elem = inner
    return elem


def _source(node, spec):
    """(address of the first element, element type, count) for an `items` or `text` field
    (schema §4), or None if it can't be resolved."""
    src = _field(node, spec["field"])
    if src is None:
        return None
    lf = _field(node, spec["len"]) if spec.get("len") else None
    if spec.get("len") and lf is None:
        return None
    return _source_at(src, lf.GetValueAsUnsigned() if lf is not None else None)


def _source_at(src, count):
    """Like _source, for a value already found. `count` is the `len` value or None."""
    t = src.GetType()
    if (t.GetName() or "").startswith("alloc::vec::Vec<"):
        elem = t.GetTemplateArgumentType(0)
        base = _first_ptr(src).GetValueAsUnsigned()
        n = src.GetChildMemberWithName("len").GetValueAsUnsigned()
        if count is not None:
            n = min(n, count)
    elif t.IsArrayType():  # [T; N] stored in place (inline buffers)
        elem = t.GetArrayElementType()
        base = src.GetLoadAddress()
        if base == lldb.LLDB_INVALID_ADDRESS or elem.GetByteSize() == 0:
            return None
        n = t.GetByteSize() // elem.GetByteSize()
        if count is not None:
            n = min(n, count)
    else:
        ptr = src if t.IsPointerType() else _first_ptr(src)  # *T or NonNull<T>
        elem = ptr.GetType().GetPointeeType()
        if count is None:
            return None
        base, n = ptr.GetValueAsUnsigned(), count
    elem = _unwrap_transparent(elem)
    if not elem.IsValid() or elem.GetByteSize() == 0:
        return None
    return base, elem, n


def _items(parent, node, spec):
    src = _source(node, spec)
    if src is None:
        return []
    base, elem, n = src
    size = elem.GetByteSize()
    return [parent.CreateValueFromAddress("[%d]" % i, base + i * size, elem)
            for i in range(min(n, ITEMS_MAX))]


def _elements(parent, base, elem, n):
    """Children `[0]`, `[1]`, ...; unreadable ones say `<unavailable>`, as in GDB."""
    size, process, kids = elem.GetByteSize(), parent.GetProcess(), []
    for i in range(min(n, ITEMS_MAX)):
        err = lldb.SBError()
        process.ReadMemory(base + i * size, min(size, 8), err)
        if err.Success():
            kids.append(parent.CreateValueFromAddress("[%d]" % i, base + i * size, elem))
        else:
            kids.append(_message(parent, "[%d]" % i, "<unavailable>"))
    return kids


def _path(v, segments):
    """Follow names from `v` (designs 0002 and 0003): fields, through unions and transparent
    wrappers, and enum variants, which only lead somewhere while active. None if a step
    fails or names an inactive variant."""
    for seg in segments:
        raw = v.GetNonSyntheticValue()
        if raw.GetChildMemberWithName("$variants$").IsValid():  # an enum: seg is a variant
            try:
                v, name = _active_variant(raw)
            except ValueError:
                return None
            if name != seg:
                return None
            continue
        v = _field(raw, seg)
        if v is None:
            return None
        t = v.GetType()
        inner = _unwrap_transparent(t)
        if inner.GetName() != t.GetName():
            addr = v.GetLoadAddress()
            if addr == lldb.LLDB_INVALID_ADDRESS:
                return None
            v = v.CreateValueFromAddress(v.GetName() or "v", addr, inner)
    return v


def _alternative(root, alts):
    """((address, element type, count), None) from the first alternative whose paths exist in
    `root` (design 0003), or (None, the message to show), as in the GDB runtime."""
    for alt in alts:
        src = _path(root, alt["items"])
        if src is None or not src.IsValid():
            continue
        count = None
        if alt.get("len"):
            lv = _path(root, alt["len"])
            if lv is None or not lv.IsValid() or lv.GetError().Fail():
                continue
            if not lv.GetType().GetTypeFlags() & lldb.eTypeIsScalar:
                return None, "<items: len is not an integer>"
            count = lv.GetValueAsUnsigned()
        try:
            found = _source_at(src, count)
        except ValueError:  # e.g. the path reached a plain integer, not a collection
            found = None
        return (found, None) if found is not None else (None, "<unsupported items source>")
    return None, "<items: no alternative matched>"


def _count(raw, d, node, vd):
    """`{#}`: the number of element children shown (design 0003), or None."""
    if d.get("alternatives"):
        src, _ = _alternative(raw, d["alternatives"])
        return None if src is None else min(src[2], ITEMS_MAX)
    if vd.get("items"):
        src = _source(node, vd["items"])
        return None if src is None else min(src[2], ITEMS_MAX)
    if vd.get("slots"):
        names = [k.GetName() for k in _slots(raw, node, vd["slots"])]
        # an error row means the count is not known; never show a confident wrong number
        return None if "[..]" in names else len(names)
    return None


def _variant_names(el):
    """Variant names of an enum element (all of them, active or not), or None if `el` is
    not an enum."""
    variants = el.GetNonSyntheticValue().GetChildMemberWithName("$variants$")
    if not variants.IsValid():
        return None
    names = []
    for i in range(variants.GetNumChildren()):
        value = variants.GetChildAtIndex(i).GetChildMemberWithName("value")
        names.append(_variant_name(value.GetType().GetName()))
    return names


def _keep(el, only):
    """(keep the element?, where `value` paths start), per design 0002 §2.1. Raises
    ValueError with the message to show when the `only` path can't be followed."""
    path, mask = only["path"], only.get("mask")
    if len(path) == 1 and mask is None and _variant_names(el) is not None:
        variant, name = _active_variant(el.GetNonSyntheticValue())  # an enum
        return name == path[0], variant
    v = _path(el, path)
    if v is None or not v.IsValid() or v.GetError().Fail():
        raise ValueError("<only: no field `%s`>" % ".".join(path))
    if not v.GetType().GetTypeFlags() & lldb.eTypeIsScalar:
        raise ValueError("<only: `%s` is not an integer>" % ".".join(path))
    n = v.GetValueAsUnsigned()  # 64-bit
    return (n & mask if mask else n) != 0, el


def _message(parent, name, text):
    """A child that shows `text` (as a quoted string: LLDB has no other way to show text)."""
    target = parent.GetTarget()
    data = lldb.SBData()
    raw = text.encode("utf-8")
    data.SetData(lldb.SBError(), raw, target.GetByteOrder(), target.GetAddressByteSize())
    char_n = target.GetBasicType(lldb.eBasicTypeChar).GetArrayType(len(raw))
    return parent.CreateValueFromData(name, data, char_n)


def _slots(parent, node, spec):
    """Children for a `slots` field: `items` with `only` / `value` (design 0002)."""
    src = _source(node, spec)
    if src is None:
        return []
    base, elem, n = src
    size = elem.GetByteSize()
    only, value = spec.get("only"), spec.get("value")
    kids = []
    process = parent.GetProcess()
    for i in range(min(n, ITEMS_MAX)):
        el = parent.CreateValueFromAddress("[%d]" % i, base + i * size, elem)
        start = el
        err = lldb.SBError()
        process.ReadMemory(base + i * size, min(size, 8), err)
        if not err.Success():  # as in GDB: say so, never drop it or blame a field
            kids.append(_message(parent, "[%d]" % i, "<unavailable>"))
            continue
        if only:
            if i == 0 and len(only["path"]) == 1 and "mask" not in only:
                names = _variant_names(el)
                if names is not None and only["path"][0] not in names:
                    return [_message(parent, "[..]", "<only: no variant `%s`>" % only["path"][0])]
            try:
                keep, start = _keep(el, only)
            except ValueError as e:
                if not any(k.GetName() == "[..]" for k in kids):
                    kids.append(_message(parent, "[..]", str(e)))
                continue
            if not keep:
                continue
        if value is None:
            kids.append(el)
            continue
        v = _path(start, value)
        addr = v.GetLoadAddress() if v is not None else lldb.LLDB_INVALID_ADDRESS
        if addr == lldb.LLDB_INVALID_ADDRESS:
            kids.append(_message(parent, "[%d]" % i, "<unavailable>"))
        else:
            kids.append(parent.CreateValueFromAddress("[%d]" % i, addr, v.GetType()))
    return kids


def _text(parent, node, spec, name):
    """A `text` field as (a `char[n]` value holding its first TEXT_MAX bytes, whether more
    were cut off), or None if it can't be read. LLDB's own summary for the value is a
    Rust-like literal, `"h\\xffi \\"\\n"`, except that it drops trailing NUL bytes."""
    src = _source(node, spec)
    if src is None or src[1].GetByteSize() != 1:
        return None
    base, _, n = src
    k = min(n, TEXT_MAX)
    target = parent.GetTarget()
    err = lldb.SBError()
    raw = parent.GetProcess().ReadMemory(base, k, err) if k else b"\0"  # char[0] has no summary
    if not err.Success():
        return None
    data = lldb.SBData()
    data.SetData(err, raw, target.GetByteOrder(), target.GetAddressByteSize())
    char_n = target.GetBasicType(lldb.eBasicTypeChar).GetArrayType(len(raw))
    return parent.CreateValueFromData(name, data, char_n), n > k


def _text_summary(parent, node, spec):
    t = _text(parent, node, spec, "text")
    s = t[0].GetSummary() if t is not None else None
    if not s:
        return "<unavailable>"
    return _clip(s + "…" if t[1] else s)


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
        texts = {x["field"]: x for x in vd.get("text", ())}
        count = None
        if any(k == "count" for k, _ in parts):
            count = _count(raw, d, node, vd)
        return "".join(
            t if k == "lit"
            else ("<unavailable>" if count is None else str(count)) if k == "count"
            else _text_summary(raw, node, texts[t]) if t in texts
            else _render(_field(node, t))
            for k, t in parts
        )
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
            slots = vd.get("slots")
            for spec in (items, slots):
                if spec:
                    hide.add(spec["field"])
            texts = {x["field"]: x for x in vd.get("text", ())}
            for i in range(node.GetNumChildren()):
                c = node.GetChildAtIndex(i)
                n = c.GetName() or ""
                if n.startswith("$"):
                    continue
                src = n[2:] if n.startswith("__") and n[2:].isdigit() else n
                if src in hide:
                    continue
                label = rename.get(src, src)
                if src in texts:
                    t = _text(self.valobj, node, texts[src], label)
                    if t is not None:  # else fall back to the raw field
                        self.kids.append(t[0])
                        continue
                self.kids.append(c.Clone(label) if label != n else c)
            if items:
                self.kids.extend(_items(self.valobj, node, items))
            elif slots:
                self.kids.extend(_slots(self.valobj, node, slots))
            elif d.get("alternatives"):
                src, message = _alternative(raw, d["alternatives"])
                if src is None:
                    self.kids.append(_message(self.valobj, "[..]", message))
                else:
                    self.kids.extend(_elements(self.valobj, *src))
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
