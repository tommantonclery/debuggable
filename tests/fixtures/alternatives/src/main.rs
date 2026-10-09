//! `items` alternatives and `{#}` (design 0003): the layouts of smallvec (default build) and
//! tinyvec, copied in so the fixture has no dependencies. Each has inline and heap instances.
#![allow(dead_code)]
use debuggable::Debuggable;
use std::marker::PhantomData;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ptr::NonNull;

pub trait Array {
    type Item;
}

impl<T, const N: usize> Array for [T; N] {
    type Item = T;
}

// ---- smallvec 1 (without the `union` feature) -------------------------------------------

pub enum SmallVecData<A: Array> {
    Inline(MaybeUninit<A>),
    Heap { ptr: NonNull<A::Item>, len: usize },
}

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

impl<T, const N: usize> SmallVec<[T; N]> {
    /// Inline: `capacity` holds the length.
    fn inline(items: Vec<T>) -> Self {
        assert!(items.len() <= N);
        let mut buf = MaybeUninit::<[T; N]>::uninit();
        let len = items.len();
        for (i, item) in items.into_iter().enumerate() {
            // SAFETY: i < len <= N, so the write stays inside `buf`.
            unsafe { buf.as_mut_ptr().cast::<T>().add(i).write(item) };
        }
        SmallVec { capacity: len, data: SmallVecData::Inline(buf), _marker: PhantomData }
    }

    /// On the heap: `capacity` holds the allocation's capacity, which is above N.
    fn heap(items: Vec<T>) -> Self {
        let mut v = ManuallyDrop::new(items);
        v.reserve(N + 1);
        let ptr = NonNull::new(v.as_mut_ptr()).unwrap();
        SmallVec { capacity: v.capacity(), data: SmallVecData::Heap { ptr, len: v.len() }, _marker: PhantomData }
    }
}

// A misspelled variant in the alternative that would be active: nothing matches.
#[derive(Debuggable)]
#[debuggable(summary = "{#} items")]
#[debuggable(items = "data.Inline.0", len = "capacity")]
#[debuggable(items = "data.Hep.ptr", len = "data.Heap.len")]
pub struct TypoVec<A: Array> {
    #[debuggable(hide)]
    capacity: usize,
    #[debuggable(hide)]
    data: SmallVecData<A>,
}

// ---- tinyvec 1 --------------------------------------------------------------------------

pub struct ArrayVec<A> {
    len: u16,
    data: A,
}

#[derive(Debuggable)]
#[debuggable(items = "Inline.0.data", len = "Inline.0.len")]
#[debuggable(items = "Heap.0")]
pub enum TinyVec<A: Array> {
    #[debuggable(summary = "{#} items (inline)")]
    Inline(#[debuggable(hide)] ArrayVec<A>),
    #[debuggable(summary = "{#} items")]
    Heap(#[debuggable(hide)] Vec<A::Item>),
}

// Paths the derive can't fully check: an alternative that reaches a plain integer, and a `len`
// that reaches a struct. Each must show one error row, never an empty or "0 items" view.
#[derive(Debuggable)]
#[debuggable(summary = "{#} items")]
#[debuggable(items = "capacity")]
pub struct WrongSource {
    capacity: usize,
}

pub struct Meta {
    n: u32,
}

#[derive(Debuggable)]
#[debuggable(summary = "{#} items")]
#[debuggable(items = "v", len = "w")]
pub struct BadLen {
    #[debuggable(hide)]
    v: Vec<u32>,
    #[debuggable(hide)]
    w: Meta,
}

// ---- inside another type's summary ----------------------------------------------------

#[derive(Debuggable)]
#[debuggable(summary = "v={v}")]
pub struct Holder {
    v: SmallVec<[u32; 4]>,
}

pub struct Fixture {
    pub small_inline: SmallVec<[u32; 4]>,
    pub small_heap: SmallVec<[String; 2]>,
    pub small_empty: SmallVec<[u32; 4]>,
    pub zero_inline: SmallVec<[u32; 0]>,
    pub tiny_inline: TinyVec<[u8; 4]>,
    pub tiny_heap: TinyVec<[String; 2]>,
    pub tiny_array: [TinyVec<[u8; 2]>; 2],
    pub dangling_heap: SmallVec<[u32; 1]>,
    pub typo: TypoVec<[u32; 1]>,
    pub holder: Holder,
    pub wrong_source: WrongSource,
    pub bad_len: BadLen,
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

fn strings(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn main() {
    let typo_heap = SmallVec::<[u32; 1]>::heap(vec![4, 5]);
    let f = Fixture {
        small_inline: SmallVec::inline(vec![1, 2, 3]),
        small_heap: SmallVec::heap(strings(&["x", "y", "z"])),
        small_empty: SmallVec::inline(vec![]),
        zero_inline: SmallVec::heap(vec![7, 8]),
        tiny_inline: TinyVec::Inline(ArrayVec { len: 2, data: [10, 20, 0, 0] }),
        tiny_heap: TinyVec::Heap(strings(&["p", "q"])),
        tiny_array: [TinyVec::Inline(ArrayVec { len: 1, data: [7, 0] }), TinyVec::Heap(vec![8, 9])],
        dangling_heap: SmallVec {
            capacity: 2,
            data: SmallVecData::Heap { ptr: NonNull::new(0x10 as *mut u32).unwrap(), len: 2 },
            _marker: PhantomData,
        },
        typo: TypoVec { capacity: typo_heap.capacity, data: typo_heap.data },
        holder: Holder { v: SmallVec::inline(vec![4, 5, 6]) },
        wrong_source: WrongSource { capacity: 3 },
        bad_len: BadLen { v: vec![1, 2], w: Meta { n: 1 } },
    };
    debuggable_fixture_stop(&f);
}
