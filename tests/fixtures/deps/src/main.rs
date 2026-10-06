//! Types from dependency crates (editions 2021 and 2024, both forbid(unsafe_code)).
#![allow(dead_code)]

pub struct Fixture { pub temp: lib2021::Celsius, pub pair: lib2021::Pair<u8, char>, pub flag: lib2024::Flag }

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
    let f = Fixture { temp: lib2021::Celsius(21.5), pair: lib2021::Pair { a: 1, b: 'z' }, flag: lib2024::Flag { on: true } };
    debuggable_fixture_stop(&f);
}
