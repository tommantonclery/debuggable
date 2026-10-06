//! `items`: Vec with and without `len`, NonNull + len, empty, and generics with two
//! instantiations of the same type.
#![allow(dead_code)]
use debuggable::Debuggable;
use std::marker::PhantomData;
use std::ptr::NonNull;

#[derive(Debuggable)]
#[debuggable(summary = "v{version} {value}")]
pub struct Slot<V> {
    pub value: Option<V>,
    #[debuggable(hide)]
    pub version: u32,
}

// the brief's example
#[derive(Debuggable)]
#[debuggable(summary = "{len} items")]
pub struct SlotMap<K, V> {
    #[debuggable(items, len = "len")]
    slots: Vec<Slot<V>>,
    #[debuggable(hide)]
    free_head: u32,
    #[debuggable(hide)]
    len: u32,
    _k: PhantomData<K>,
}

#[derive(Debuggable)]
pub struct Stack<T> {
    #[debuggable(items)]
    pub items: Vec<T>,
}

// raw-parts collection
#[derive(Debuggable)]
#[debuggable(summary = "{len}/{cap}")]
pub struct RawBuf {
    #[debuggable(items, len = "len")]
    ptr: NonNull<u16>,
    len: usize,
    cap: usize,
}

pub struct Fixture {
    pub map_str: SlotMap<u8, String>,
    pub map_int: SlotMap<u8, i32>,
    pub stack: Stack<u16>,
    pub empty: Stack<u16>,
    pub raw: RawBuf,
    backing: Vec<u16>,
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

fn main() {
    let mut map_str = SlotMap { slots: Vec::with_capacity(8), free_head: 3, len: 2, _k: PhantomData };
    map_str.slots.push(Slot { value: Some(String::from("alpha")), version: 1 });
    map_str.slots.push(Slot { value: Some(String::from("beta")), version: 3 });
    map_str.slots.push(Slot { value: None, version: 2 }); // beyond len: must not be shown
    let map_int = SlotMap { slots: vec![Slot { value: Some(42), version: 1 }], free_head: 0, len: 1, _k: PhantomData };
    let mut backing = vec![10u16, 20, 30, 40];
    let raw = RawBuf { ptr: NonNull::new(backing.as_mut_ptr()).unwrap(), len: 3, cap: 4 };
    let f = Fixture {
        map_str,
        map_int,
        stack: Stack { items: vec![5, 6] },
        empty: Stack { items: Vec::new() },
        raw,
        backing,
    };
    debuggable_fixture_stop(&f);
}
