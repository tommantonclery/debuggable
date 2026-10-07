# Slot collections: `only` and `value` on `items`

**Status:** design approved in conversation 2026-10-07; this document awaits review.
**Target release:** 0.1.2 (additive: new attribute options and a new descriptor key).

## 1. Problem

`items` shows every element of a `Vec`, array or pointer + length. Slot collections store their
values in a `Vec` of slots, some of them vacant, so `items` today shows every slot, vacant ones
included, wrapped in the slot type. Reading a vacant slot's value can show stale or
uninitialized memory as if it were live.

| Crate | Field | Slot type | Occupied when | Value |
|---|---|---|---|---|
| slab 0.4 | `entries: Vec<Entry<T>>` | `enum Entry { Vacant(usize), Occupied(T) }` | variant `Occupied` | `0` of the variant |
| generational-arena 0.2 | `items: Vec<Entry<T>>` | `enum Entry { Free { next_free }, Occupied { generation, value } }` | variant `Occupied` | `value` of the variant |
| slotmap 1 (`SlotMap`, `HopSlotMap`) | `slots: Vec<Slot<V>>` | `struct Slot { u: union { value: ManuallyDrop<T>, next_free: u32 }, version: u32 }` | `version` is odd | `u.value` |

Inline-or-heap collections (smallvec, tinyvec) are a different problem and are out of scope
(§8).

## 2. User-facing syntax

Two new options, allowed only together with `items` on the same field:

```rust
// slab
#[debuggable(items, only = "Occupied", value = "0")]
entries: Vec<Entry<T>>,

// generational-arena
#[debuggable(items, only = "Occupied", value = "value")]
items: Vec<Entry<T>>,

// slotmap
#[debuggable(items, only = "version & 1", value = "u.value")]
slots: Vec<Slot<V>>,
```

In a debugger: `map = 2 items { [0] = "a", [3] = "d" }`. The summary is the type's own
`summary`, for example `"{num_elems} items"`.

### 2.1 `only = "..."`: which elements to keep

```
only  := path [ "&" mask ]
path  := segment ( "." segment )*
segment := identifier | decimal digits        (tuple fields: "0", "1", ...)
mask  := decimal integer | "0x" hex integer   (1 ..= 2^53 - 1, see §4)
```

Whitespace around `&` is allowed. The runtime evaluates it per element:

- **Single segment, no mask, element is an enum:** keep the element if its active variant is
  named `segment`.
- **Otherwise:** follow `path` through the element's fields (unions included; transparent
  wrappers unwrapped after each step) to an integer or bool. Keep the element if the value is
  non-zero, or, with a mask, if `value & mask != 0`.
- If the path can't be followed for an element, that element is **not** kept, and the first
  such failure is reported once as a child `[..] = <only: no field `x`>`.

No negation, comparison or other operators. They can be added later without breaking anything.

### 2.2 `value = "path"`: what to show for a kept element

- Same `path` grammar as `only`.
- The path starts at the active variant's fields when `only` selected a variant, and at the
  element otherwise.
- Transparent wrappers (`MaybeUninit`, `ManuallyDrop`, `MaybeDangling`) are unwrapped after each
  step and at the end.
- If the path can't be followed for an element, that child shows `<unavailable>`.
- Without `value`, the whole element is shown (as today).

### 2.3 Labels, counts and limits

- A kept element keeps its **original index** as its label (`[3]`), because for slot
  collections the index is part of the key.
- `len` keeps its current meaning: it bounds the number of elements **scanned**.
- The 10,000 limit applies to elements scanned.
- `only` without `value`, and `value` without `only`, are both allowed.

## 3. Compile-time validation (derive)

Errors, each with a span on the offending string or key:

- `only` or `value` without `items` on the same field: "`only` needs `items` on the same field".
- Duplicate `only` or `value`.
- Syntax: empty path, empty segment (`a..b`, `.a`), a segment that is neither an identifier nor
  digits, `&` without a mask, a mask of zero, a mask above 2^53 - 1, trailing input.
  For example: "`only` expects a field or variant name, optionally `& mask`: `\"version & 1\"`".
- `r#` prefixes on segments are stripped, as for field names elsewhere.

The derive cannot check that names exist in the element type: it is usually generic or defined
elsewhere. That is a documented limitation; wrong names show at debug time as in §2.1/§2.2.

## 4. Descriptor schema (addition to schema v1 §4)

A filtered `items` field is emitted under a **new key, `slots`**, instead of `items`:

```jsonc
"slots": {
  "field": "slots",
  "len": "len",                                   // optional, as for items
  "only": {"path": ["version"], "mask": 1},       // optional; "mask" optional
  "value": ["u", "value"]                         // optional
}
```

A field with plain `items` (neither option) still emits `items`, byte for byte as today.

Why a new key: schema v1 §7 allows additive changes only when an older runtime can safely
ignore them. An older runtime that saw `"items"` with extra keys would show every slot,
vacant ones included, as live values: a misrender. Under `slots`, an older runtime ignores
the key and shows the field unformatted, which is honest. GDB always runs the newest runtime
in the binary, so only a stale LLDB loader can hit this, and `doctor` already reports stale
loaders.

Runtimes treat `slots` exactly like `items` (it hides the field and appends children) with the
filtering and projection of §2. At most one of `items` or `slots` is present.

Masks are JSON integers. The derive rejects masks above 2^53 - 1 so every JSON parser reads
them exactly; that covers every flag bit in a u32 and the lower 53 bits of a u64.

## 5. Runtime changes

Both runtimes already resolve the source (`_source`) and unwrap transparent wrappers. New:

- `_path(value, segments)`: follow fields, mapping `"0"` to DWARF `__0`, unwrapping transparent
  wrappers after each step. Returns None when a step fails.
- `_keep(element, only)`: §2.1. For enums, reuse `_active_variant` (which in GDB re-reads
  through the address, because elements reached through arrays arrive unresolved).
- In the items loop: read element i; if `only` and not kept, skip it; otherwise show
  `_path(start, value)` (or the element) as `[i]`.
- GDB runtime minor 6. LLDB loader 1.4.

Performance: one extra field read per scanned element. The scan is bounded by `len` and the
10,000 limit, as today.

## 6. Documentation

- Crate docs: a subsection under `items` with the three examples from §2.
- README attribute table: `only` and `value` rows.
- `docs/internal/schema-v1.md`: the `slots` key, §2.1/§2.2 semantics, size table.
- CHANGELOG entry for 0.1.2.

## 7. Tests

- **Fixture `slots`** (no new dependencies): the slab, generational-arena and slotmap layouts
  copied in, with `#[derive(Debuggable)]` and the attributes of §2. Each has vacant slots
  before, between and after occupied ones, values of `String` and `u32`, and one empty
  collection. slotmap's vacant slots hold a `next_free` in the union, so showing them by
  mistake would print an obviously wrong value.
- One more case for `only` on a struct field without a mask (a `bool` `live` flag) and
  `value` without `only`.
- Snapshots for GDB 15, LLDB 18, LLDB 20 (here), LLDB 22 and Apple LLDB (CI artifacts), in all
  four profiles.
- Derive unit tests for the JSON and every error in §3; one UI test for a span.
- `doctor`'s type-name check covers the new fixture automatically.
- Dogfood: annotate a local copy of slab and check `Slab<String>` in rust-gdb and LLDB.

## 8. Out of scope

- Inline-or-heap collections (smallvec, tinyvec), which need a source chosen by a condition.
- `items` inside enum variants.
- Negation and comparisons in `only`.
