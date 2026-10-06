//! Structs: summary, hide, rename, tuple fields, PhantomData auto-hide, nested summaries.
//! `__entry!` calls stand in for `#[derive(Debuggable)]` until Phase 4.
#![allow(dead_code)]
use debuggable::__entry;
use std::marker::PhantomData;

pub mod geo {
    // #[debuggable(summary = "({x}, {y})")]
    pub struct Point { pub x: i32, pub y: i32 }
    debuggable::__entry!("Point", r#""generic":false,"kind":"struct","summary":[["lit","("],["field","x"],["lit",", "],["field","y"],["lit",")"]]}"#);
}

// #[debuggable(summary = "{0} m")]
pub struct Meters(pub f64);
__entry!("Meters", r#""generic":false,"kind":"struct","summary":[["field","0"],["lit"," m"]]}"#);

// no summary; #[debuggable(hide)] secret; #[debuggable(rename = "balance_pence")] balance; _p auto-hidden
pub struct Account { pub id: u32, pub secret: u64, pub balance: i64, pub _p: PhantomData<u8> }
__entry!("Account", r#""generic":false,"kind":"struct","hide":["secret","_p"],"rename":{"balance":"balance_pence"}}"#);

// #[debuggable(summary = "{from} +{dist}")] -- summaries of other debuggable types
pub struct Trip { pub from: geo::Point, pub dist: Meters }
__entry!("Trip", r#""generic":false,"kind":"struct","summary":[["field","from"],["lit"," +"],["field","dist"]]}"#);

// a type with no entry at all: must render exactly as the debugger normally would
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
