//! `items` with `only` / `value` (design 0002): the slot layouts of slab, generational-arena
//! and slotmap, copied in so the fixture has no dependencies. Every collection has vacant
//! slots before, between and after occupied ones; showing a vacant slot would be visible.
#![allow(dead_code)]
use debuggable::Debuggable;
use std::mem::ManuallyDrop;

// ---- slab 0.4 ---------------------------------------------------------------------------

pub enum Entry<T> {
    Vacant(usize),
    Occupied(T),
}

#[derive(Debuggable)]
#[debuggable(summary = "{#} of {len} items")]
pub struct Slab<T> {
    #[debuggable(items, only = "Occupied", value = "0")]
    entries: Vec<Entry<T>>,
    #[debuggable(hide)]
    len: usize,
}

impl<T> Slab<T> {
    fn new(slots: Vec<Option<T>>) -> Self {
        let len = slots.iter().filter(|s| s.is_some()).count();
        let entries = slots.into_iter().map(|s| s.map_or(Entry::Vacant(99), Entry::Occupied)).collect();
        Slab { entries, len }
    }
}

// A misspelled variant: must say so, not look like an empty collection.
#[derive(Debuggable)]
#[debuggable(summary = "{#} items")]
pub struct TypoSlab<T> {
    #[debuggable(items, only = "Ocupied", value = "0")]
    entries: Vec<Entry<T>>,
}

// ---- generational-arena 0.2 -------------------------------------------------------------

pub enum ArenaEntry<T> {
    Free { next_free: Option<usize> },
    Occupied { generation: u64, value: T },
}

#[derive(Debuggable)]
#[debuggable(summary = "({x}, {y})")]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debuggable)]
#[debuggable(summary = "{len} items")]
pub struct Arena<T> {
    #[debuggable(items, only = "Occupied", value = "value")]
    items: Vec<ArenaEntry<T>>,
    #[debuggable(hide)]
    generation: u64,
    #[debuggable(hide)]
    len: usize,
}

// ---- slotmap 1 --------------------------------------------------------------------------

pub union SlotUnion<T> {
    value: ManuallyDrop<T>,
    next_free: u32,
}

pub struct Slot<T> {
    u: SlotUnion<T>,
    version: u32, // even = vacant, odd = occupied
}

#[derive(Debuggable)]
#[debuggable(summary = "{num_elems} items")]
pub struct SlotMap<V> {
    #[debuggable(items, only = "version & 1", value = "u.value")]
    slots: Vec<Slot<V>>,
    #[debuggable(hide)]
    free_head: u32,
    #[debuggable(hide)]
    num_elems: u32,
}

// ---- other shapes -----------------------------------------------------------------------

// A mask above bit 31 of a u64.
pub struct Row {
    flags: u64,
    id: u32,
}

#[derive(Debuggable)]
pub struct Flagged {
    #[debuggable(items, only = "flags & 0x100000000", value = "id")]
    rows: Vec<Row>,
}

// `only` on a bool field, without `value`: whole elements.
pub struct Cell {
    live: bool,
    v: u8,
}

#[derive(Debuggable)]
pub struct Live {
    #[debuggable(items, only = "live")]
    cells: Vec<Cell>,
}

// `value` without `only`: every element, projected.
pub struct W {
    inner: u16,
    pad: u16,
}

#[derive(Debuggable)]
pub struct Wrapped {
    #[debuggable(items, value = "inner")]
    w: Vec<W>,
}

// `only` paths that are wrong in ways the derive can't see: a misspelled field, and a path
// that ends at a struct rather than an integer (like a `NonZeroU32` version).
pub struct Meta {
    flags: u32,
}

pub struct Tagged {
    meta: Meta,
    id: u32,
}

#[derive(Debuggable)]
pub struct NonScalar {
    #[debuggable(items, only = "meta", value = "id")]
    rows: Vec<Tagged>,
}

#[derive(Debuggable)]
pub struct TypoField {
    #[debuggable(items, only = "flgas & 1", value = "id")]
    rows: Vec<Row>,
}

// Unreadable elements (a dangling pointer): each must show `<unavailable>`, never vanish.
#[derive(Debuggable)]
pub struct Dangling {
    #[debuggable(items, len = "n", only = "Occupied", value = "0")]
    p: *const Entry<u32>,
    n: usize,
}

#[derive(Debuggable)]
pub struct DanglingRows {
    #[debuggable(items, len = "n", only = "flags & 1", value = "id")]
    p: *const Row,
    n: usize,
}

pub struct Fixture {
    pub slab: Slab<String>,
    pub arena: Arena<Point>,
    pub slotmap: SlotMap<String>,
    pub flagged: Flagged,
    pub typo: TypoSlab<u32>,
    pub all_vacant: Slab<String>,
    pub empty: Slab<String>,
    pub live: Live,
    pub wrapped: Wrapped,
    pub nonscalar: NonScalar,
    pub typo_field: TypoField,
    pub dangling: Dangling,
    pub dangling_rows: DanglingRows,
}

/// The harness breaks here and prints `f`'s fields. The empty `asm!` keeps `f` live and its
/// pointee in memory without a call: `black_box` would be inlined in release builds, and the
/// breakpoint would then land in its inlined frame, where `f` is not in scope.
#[no_mangle]
#[inline(never)]
pub fn debuggable_fixture_stop(f: &Fixture) {
    // SAFETY: the template is a comment; no instructions are emitted.
    unsafe { std::arch::asm!("/* {0} */", in(reg) f as *const Fixture, options(nostack, preserves_flags)) }
}

fn s(x: &str) -> Option<String> {
    Some(x.to_string())
}

fn main() {
    let free = |next: u32, version: u32| Slot { u: SlotUnion { next_free: next }, version };
    let used = |v: &str, version: u32| Slot { u: SlotUnion { value: ManuallyDrop::new(v.to_string()) }, version };
    let f = Fixture {
        slab: Slab::new(vec![None, s("a"), None, s("d"), None]),
        arena: Arena {
            items: vec![
                ArenaEntry::Free { next_free: Some(2) },
                ArenaEntry::Occupied { generation: 1, value: Point { x: 1, y: 2 } },
                ArenaEntry::Free { next_free: None },
                ArenaEntry::Occupied { generation: 3, value: Point { x: -3, y: 4 } },
                ArenaEntry::Free { next_free: Some(0) },
            ],
            generation: 3,
            len: 2,
        },
        slotmap: SlotMap {
            slots: vec![free(4, 2), used("x", 1), free(0, 4), used("y", 3), free(2, 0)],
            free_head: 0,
            num_elems: 2,
        },
        flagged: Flagged {
            rows: vec![
                Row { flags: 1, id: 10 },        // bit 0 only: a 32-bit mask would wrongly keep it
                Row { flags: 1 << 32, id: 11 },
                Row { flags: 0, id: 12 },
                Row { flags: (1 << 32) | 1, id: 13 },
            ],
        },
        typo: TypoSlab { entries: vec![Entry::Vacant(1), Entry::Occupied(5)] },
        all_vacant: Slab::new(vec![None, None, None]),
        empty: Slab::new(vec![]),
        live: Live { cells: vec![Cell { live: false, v: 1 }, Cell { live: true, v: 2 }, Cell { live: false, v: 3 }] },
        wrapped: Wrapped { w: vec![W { inner: 7, pad: 0 }, W { inner: 8, pad: 0 }] },
        nonscalar: NonScalar { rows: vec![Tagged { meta: Meta { flags: 1 }, id: 1 }, Tagged { meta: Meta { flags: 0 }, id: 2 }] },
        typo_field: TypoField { rows: vec![Row { flags: 1, id: 1 }, Row { flags: 0, id: 2 }] },
        dangling: Dangling { p: 0x10 as *const Entry<u32>, n: 2 },
        dangling_rows: DanglingRows { p: 0x10 as *const Row, n: 2 },
    };
    debuggable_fixture_stop(&f);
}
