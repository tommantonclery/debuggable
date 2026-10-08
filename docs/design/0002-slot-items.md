# 0002: Slot collections (`only` and `value` on `items`)

**Status:** implemented in 0.1.2 (GDB runtime 1.6, LLDB loader 1.4). The current contract is
`docs/internal/schema-v1.md` §4.

## Problem

`items` shows every element of a `Vec`, array or pointer + length. Slot collections keep their
values in a `Vec` of slots, some of them vacant, so `items` alone shows every slot, wrapped in the
slot type. Worse, reading a vacant slot's value can show stale or uninitialized memory as if it
were live.

| Crate | Field | Slot type | Occupied when | Value |
|---|---|---|---|---|
| slab 0.4 | `entries: Vec<Entry<T>>` | `enum Entry { Vacant(usize), Occupied(T) }` | variant `Occupied` | `0` of the variant |
| generational-arena 0.2 | `items: Vec<Entry<T>>` | `enum Entry { Free { .. }, Occupied { generation, value } }` | variant `Occupied` | `value` of the variant |
| slotmap 1 (`SlotMap`, `HopSlotMap`) | `slots: Vec<Slot<V>>` | `struct Slot { u: union { value: ManuallyDrop<T>, next_free: u32 }, version: u32 }` | `version` is odd | `u.value` |

## Decision

Two options on `items`, set on the collection's field, so the slot type needs no annotation:

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

A slab holding "a" in slot 1 and "d" in slot 3 shows as `2 items = {[1] = "a", [3] = "d"}`.

The alternative was annotating the slot type itself (`#[debuggable(vacant)]` on a variant). That
composes wherever the slot type appears, but needs two annotations per collection, and slotmap's
"odd version" rule needs a condition either way.

### `only = "..."`: which elements to keep

```
only    := path [ "&" mask ]
path    := segment ( "." segment )*
segment := identifier | decimal digits        (tuple fields: "0", "1", ...)
mask    := decimal or "0x" hex integer, 1 ..= 2^53 - 1
```

- **A single name on enum elements** compares the active variant's name.
- **Anything else** follows the path through fields (unions included; `MaybeUninit`,
  `ManuallyDrop` and `MaybeDangling` unwrapped) to an integer or bool, and keeps the element if it
  is non-zero, or if `value & mask != 0`.
- No negation or comparisons; they can be added later without breaking anything.

### `value = "path"`: what to show

A path from the element, or from the variant `only` selected. Without `value`, the whole element
is shown. `only` and `value` can each be used without the other.

### Labels and limits

Kept elements keep their **original index** (`[3]`), because for slot collections the index is
part of the key. `len` and the 10,000 limit bound the elements *scanned*, as for `items`.

## Errors

The derive checks syntax at compile time, with a span on the string: `only`/`value` without
`items`, empty or malformed segments, `&` without a mask, a zero mask, a mask above 2^53 - 1.
It can't check names inside the element type, which is usually generic or defined elsewhere, so
the runtimes report those in the debugger instead, as one row, so a mistake never looks like a
valid empty collection:

| Situation | Shown |
|---|---|
| `only` names a variant the enum doesn't have | `[..] = <only: no variant `Ocupied`>` |
| `only` names a field that doesn't exist | `[..] = <only: no field `flgas`>` |
| `only` path ends at a struct, not an integer | `[..] = <only: `meta` is not an integer>` |
| `value` path can't be followed for an element | `[i] = <unavailable>` |
| element memory can't be read | `[i] = <unavailable>` |

LLDB shows these messages in quotes, because it can only display synthesized text as a string.

## Schema

A field with `only` or `value` is emitted under a **new key, `slots`**, instead of `items`:

```jsonc
"slots": {"field": "slots", "len": "len", "only": {"path": ["version"], "mask": 1}, "value": ["u", "value"]}
```

Plain `items` is emitted exactly as before. A new key keeps the change additive within schema v1:
an older runtime seeing `items` with extra keys would show vacant slots as live values, whereas
it ignores `slots` and shows the plain field. GDB always runs the newest runtime in the binary,
so only a stale LLDB loader could hit this, and `doctor` reports stale loaders.

Masks are capped at 2^53 - 1 so every JSON parser reads them exactly. That covers every bit of a
u32 and the lower 53 bits of a u64.

## Testing

The `slots` fixture copies the three layouts above (no dependencies), with vacant slots before,
between and after occupied ones, plus cases for each row of the error table, a bit-32 mask on a
u64, an all-vacant and an empty collection, and a `value` that is itself `Debuggable`. Snapshots
cover GDB 15, LLDB 18, 20 and 22 and Apple LLDB, in all four build profiles, and all five agree.
The real slab 0.4.12 source, annotated as above, was checked in rust-gdb and LLDB.

Building this also found an LLDB bug present since 0.1.0: variant summaries on generic enums
never matched, because LLDB names variant types with their generic arguments
(`Msg<u32>::Data<u32>`). Fixed in loader 1.4, with a `Msg<T>` case in the enums fixture.

## Out of scope

- Inline-or-heap collections (smallvec, tinyvec), which need a source chosen by a condition.
- `items` inside enum variants.
