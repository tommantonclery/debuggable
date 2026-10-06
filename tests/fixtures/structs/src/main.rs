//! Structs: summary, hide, rename, tuple fields, PhantomData auto-hide, nested summaries.
#![allow(dead_code)]
use debuggable::Debuggable;
use std::marker::PhantomData;

pub mod geo {
    #[derive(debuggable::Debuggable)]
    #[debuggable(summary = "({x}, {y})")]
    pub struct Point { pub x: i32, pub y: i32 }
}

#[derive(Debuggable)]
#[debuggable(summary = "{0} m")]
pub struct Meters(pub f64);

// no summary; `_p` is hidden automatically
#[derive(Debuggable)]
pub struct Account {
    pub id: u32,
    #[debuggable(hide)]
    pub secret: u64,
    #[debuggable(rename = "balance_pence")]
    pub balance: i64,
    pub _p: PhantomData<u8>,
}

// summaries of other debuggable types
#[derive(Debuggable)]
#[debuggable(summary = "{from} +{dist}")]
pub struct Trip { pub from: geo::Point, pub dist: Meters }

// no derive at all: must render exactly as the debugger normally would
pub struct Plain { pub a: u8 }

pub struct Fixture { pub point: geo::Point, pub meters: Meters, pub account: Account, pub trip: Trip, pub plain: Plain }

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
    let f = Fixture {
        point: geo::Point { x: 3, y: -4 },
        meters: Meters(12.5),
        account: Account { id: 7, secret: 0xDEAD_BEEF, balance: -250, _p: PhantomData },
        trip: Trip { from: geo::Point { x: 1, y: 2 }, dist: Meters(0.75) },
        plain: Plain { a: 9 },
    };
    debuggable_fixture_stop(&f);
}
