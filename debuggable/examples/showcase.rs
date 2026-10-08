//! The same data in plain types and in types that derive `Debuggable`, to compare in a debugger.
//!
//!     cargo build -p debuggable --example showcase
//!     rust-gdb target/debug/examples/showcase -ex 'break showcase::look' -ex run -ex 'p *plain' -ex 'p *pretty'
//!
//! In VS Code (CodeLLDB), set a breakpoint in `look` and expand `plain` and `pretty`.
#![allow(dead_code)]

use debuggable::Debuggable;
use std::mem::{ManuallyDrop, MaybeUninit};

/// A slot map, laid out like the `slotmap` crate: a slot is in use when its version is odd.
mod plain {
    use super::*;

    pub union SlotUnion<T> {
        pub value: ManuallyDrop<T>,
        pub next_free: u32,
    }

    pub struct Slot<T> {
        pub u: SlotUnion<T>,
        pub version: u32,
    }

    pub struct SlotMap<T> {
        pub slots: Vec<Slot<T>>,
        pub free_head: u32,
        pub num_elems: u32,
    }

    /// A fixed-capacity string, laid out like `arrayvec::ArrayString`.
    pub struct InlineString<const N: usize> {
        pub len: u8,
        pub xs: [MaybeUninit<u8>; N],
    }

    pub struct Player {
        pub name: InlineString<16>,
        pub inventory: SlotMap<String>,
    }
}

mod pretty {
    use super::*;
    pub use super::plain::{Slot, SlotUnion};

    #[derive(Debuggable)]
    #[debuggable(summary = "{num_elems} items")]
    pub struct SlotMap<T> {
        #[debuggable(items, only = "version & 1", value = "u.value")]
        pub slots: Vec<Slot<T>>,
        #[debuggable(hide)]
        pub free_head: u32,
        #[debuggable(hide)]
        pub num_elems: u32,
    }

    #[derive(Debuggable)]
    #[debuggable(summary = "{xs}")]
    pub struct InlineString<const N: usize> {
        #[debuggable(hide)]
        pub len: u8,
        #[debuggable(text, len = "len", hide)]
        pub xs: [MaybeUninit<u8>; N],
    }

    #[derive(Debuggable)]
    #[debuggable(summary = "{name}, {inventory}")]
    pub struct Player {
        pub name: InlineString<16>,
        pub inventory: SlotMap<String>,
    }
}

macro_rules! build {
    ($m:ident) => {{
        let mut xs = [MaybeUninit::uninit(); 16];
        for (slot, b) in xs.iter_mut().zip(b"ferris".iter()) {
            slot.write(*b);
        }
        let used = |v: &str, version: u32| $m::Slot {
            u: $m::SlotUnion { value: ManuallyDrop::new(v.to_string()) },
            version,
        };
        let free = |next: u32, version: u32| $m::Slot { u: $m::SlotUnion { next_free: next }, version };
        $m::Player {
            name: $m::InlineString { len: 6, xs },
            inventory: $m::SlotMap {
                slots: vec![free(2, 2), used("sword", 1), free(4, 4), used("shield", 3), used("map", 7)],
                free_head: 0,
                num_elems: 3,
            },
        }
    }};
}

/// Break here to compare the two.
#[inline(never)]
fn look(plain: &plain::Player, pretty: &pretty::Player) {
    std::hint::black_box((plain, pretty));
}

fn main() {
    let plain = build!(plain);
    let pretty = build!(pretty);
    look(&plain, &pretty);
}
