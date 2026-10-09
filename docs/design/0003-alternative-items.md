# 0003: Inline-or-heap collections (`items` alternatives and `{#}`)

**Status:** implemented in 0.1.3 (GDB runtime 1.7, LLDB loader 1.5).
The current contract is `docs/internal/schema-v1.md` §4.

## Problem

Small-vector types keep their elements inline until they outgrow the inline buffer, then move them
to the heap. `items` names a single field, so it can't follow the elements from one place to the
other.

| Crate | Where the elements are | Where the length is |
|---|---|---|
| smallvec 1 (default build) | `data: enum SmallVecData { Inline(MaybeUninit<[T; N]>), Heap { ptr, len } }` | inline: the outer field `capacity`; heap: `data.Heap.len` |
| tinyvec 1 `TinyVec` (itself an enum) | `Inline(ArrayVec { len, data: [T; N] })` or `Heap(Vec<T>)` | `Inline.0.len`, or the `Vec` |

In both crates, the active variant of an enum decides which storage holds the elements, and
debuggers already know which variant is active. So no condition syntax is needed: it's enough to
say where the elements are in each case.

## Decision

`items` can also go on the type itself, with a path. Each attribute is one place the elements can
be, and the first one that exists in the current value wins:

```rust
// smallvec
#[derive(Debuggable)]
#[debuggable(summary = "{#} items")]
#[debuggable(items = "data.Inline.0", len = "capacity")]
#[debuggable(items = "data.Heap.ptr", len = "data.Heap.len")]
pub struct SmallVec<A: Array> {
    #[debuggable(hide)]
    capacity: usize,
    #[debuggable(hide)]
    data: SmallVecData<A>,
    _marker: PhantomData<A::Item>,
}

// tinyvec: an enum, so paths start at a variant
#[derive(Debuggable)]
#[debuggable(items = "Inline.0.data", len = "Inline.0.len")]
#[debuggable(items = "Heap.0")]
pub enum TinyVec<A: Array> {
    #[debuggable(summary = "{#} items (inline)")]
    Inline(#[debuggable(hide)] ArrayVec<A>),
    #[debuggable(summary = "{#} items")]
    Heap(#[debuggable(hide)] Vec<A::Item>),
}
```

Either way, a vector holding 1, 2 and 3 shows as `v = 3 items = {[0] = 1, [1] = 2, [2] = 3}`,
whether it is inline or on the heap.

The alternative was allowing `items` inside enum variants. That works for tinyvec, but not for
smallvec, whose inline length lives outside the enum, and it adds a nesting level to the view.

The field-level form, `#[debuggable(items)]` on a field, doesn't change. A type uses one form or
the other.

### Paths

Paths use the same dotted names as `value` (see 0002), starting from the type itself:

- A segment names a field (`capacity`, `0`) or, on an enum, a **variant**. A variant segment only
  leads somewhere while that variant is active. Otherwise the path doesn't exist for this value.
- `MaybeUninit`, `ManuallyDrop` and `MaybeDangling` are unwrapped along the way, so `data.Inline.0`
  (a `MaybeUninit<[T; N]>`) reaches the array.
- What the path reaches follows the existing `items` rules: a `Vec`, an array `[T; N]`, or a
  pointer with `len`.

### Choosing an alternative

Alternatives are tried in the order written. The first one whose `items` path, and `len` path if
it has one, both exist is used.

### `{#}`: the number of elements

A smallvec on the heap has no length field to put in a summary like `"{len} items"`. `{#}` fills
that gap: it inserts the number of elements the type shows, after `len`, the 10,000 limit and, for
slot collections, `only`. It works in any summary of a type that has `items` in any form, so slot
collections can use it too, and it can't clash with a field name.

On a slot collection, counting means reading every scanned slot, so the summary costs the same as
expanding the value.

## Errors

The derive can check more here than for `only`, because the start of each path is visible in the
type. With a span on the string and the usual did-you-mean suggestion, it rejects:

- on a struct, a first segment that isn't one of its fields;
- on an enum, a first segment that isn't a variant, or a second that isn't a field of it;
- `len` without `items = "..."` in the same attribute;
- `items = "..."` on a field (there, `items` is a flag);
- a type with both field-level `items` and alternatives;
- `{#}` on a type without `items`;
- `only` or `value` together with alternatives (not supported yet).

Deeper segments usually go through private types the derive can't see, so the runtimes report
those, as one row, never as an empty view that looks valid:

| Situation | Shown |
|---|---|
| no alternative exists in the current value | `[..] = <items: no alternative matched>` |
| the matched path reaches something that isn't a `Vec`, array or pointer | `[..] = <unsupported items source>` |
| the matched `len` path reaches something that isn't an integer | `[..] = <items: len is not an integer>` |
| element memory can't be read | `[i] = <unavailable>` |

While one of these rows is shown, `{#}` shows `<unavailable>` rather than a count, and the same
goes for a slot collection showing an `only` error row. As with 0002, LLDB shows these messages
in quotes.

## Schema

Alternatives are emitted under a new key, and `{#}` as a new summary part:

```jsonc
"alternatives": [
  {"items": ["data", "Inline", "0"], "len": ["capacity"]},
  {"items": ["data", "Heap", "ptr"], "len": ["data", "Heap", "len"]}
],
"summary": [["count", ""], ["lit", " items"]]
```

On an enum, `alternatives` sits at the top of the descriptor, because its paths start at the enum,
not inside `variants`.

Both are additive within schema v1. An older runtime ignores `alternatives` and shows the plain
fields, which is honest. It shows a `count` part as `<unavailable>`, which is not wrong data. As
before, only a stale LLDB loader can meet either, and `doctor` reports stale loaders. This is GDB
runtime 1.7 and LLDB loader 1.5.

## Implementation notes

- The path-following added for `value` learns variant segments. GDB follows the field it exposes
  for the active variant, re-reading through the address when the value arrived unresolved. LLDB
  compares the segment with the active variant's name.
- Source resolution takes a resolved value instead of a field name, so field-level `items`,
  `slots` and alternatives share one code path.
- On enums, alternatives are resolved from the whole value, before the printer narrows to the
  active variant's fields.

## Testing

An `alternatives` fixture copies smallvec's default layout and tinyvec's (no dependencies), each
with an inline and a spilled instance, with `u32` and `String` elements, plus an empty one. A
misspelled variant in the alternative that would be active checks the "no alternative matched"
row, and two more types check the other error rows. `{#}` is used in struct and variant
summaries, and on a slot collection in the `slots` fixture. Snapshots
cover GDB 15, LLDB 18, 20 and 22 and Apple LLDB in all four profiles, with derive unit tests for
each error above and a UI test for a span. Annotated local copies of smallvec 1 and tinyvec 1 get
checked in rust-gdb and LLDB.

## Out of scope

- smallvec's optional `union` feature, which replaces the enum with an untagged union chosen by
  `capacity > N`. Supporting it needs a condition, which this design deliberately avoids.
- `only` and `value` together with alternatives.
- Conditions in general: alternatives are chosen only by which paths exist.
