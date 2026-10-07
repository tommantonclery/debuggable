//! `text`: byte buffers shown as strings. An inline `[MaybeUninit<u8>; N]` with `len` (as in
//! arrayvec's ArrayString), a fixed `[u8; 4]`, a `Vec<u8>` with invalid UTF-8 and characters
//! that need escaping, `NonNull<u8>` + `len`, and a summary longer than the clip limit.
#![allow(dead_code)]
use debuggable::Debuggable;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

// ArrayString's layout: bytes past `len` are uninitialized and must not be read.
#[derive(Debuggable)]
#[debuggable(summary = "{xs}")]
pub struct InlineStr<const N: usize> {
    #[debuggable(hide)]
    len: u8,
    #[debuggable(text, len = "len", hide)]
    xs: [MaybeUninit<u8>; N],
}

impl<const N: usize> InlineStr<N> {
    fn new(s: &str) -> Self {
        let mut xs = [const { MaybeUninit::uninit() }; N];
        for (slot, b) in xs.iter_mut().zip(s.bytes()) {
            slot.write(b);
        }
        InlineStr { len: s.len() as u8, xs }
    }
}

// a fixed-size code with no `len`: every byte is shown, NUL included; the child is visible
#[derive(Debuggable)]
#[debuggable(summary = "tag {code}")]
pub struct Tag {
    #[debuggable(text, rename = "name")]
    code: [u8; 4],
    pub id: u16,
}

#[derive(Debuggable)]
#[debuggable(summary = "{data}")]
pub struct Buf {
    #[debuggable(text)]
    data: Vec<u8>,
}

#[derive(Debuggable)]
#[debuggable(summary = "{len} bytes: {ptr}")]
pub struct RawText {
    #[debuggable(text, len = "len", hide)]
    ptr: NonNull<u8>,
    len: usize,
}

pub struct Fixture {
    pub name: InlineStr<16>,
    pub empty: InlineStr<8>,
    pub tag: Tag,
    pub buf: Buf,
    pub raw: RawText,
    pub long: Buf,
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

static GREETING: &[u8] = b"hi there, partial";

fn main() {
    let f = Fixture {
        name: InlineStr::new("hello"),
        empty: InlineStr::new(""),
        tag: Tag { code: *b"AB\0C", id: 7 },
        // invalid byte, quotes, a backslash, a newline, a tab, and a two-byte character
        buf: Buf { data: b"h\xffi \"q\" \\ \n\tcaf\xc3\xa9".to_vec() },
        raw: RawText { ptr: NonNull::from(GREETING).cast(), len: 8 },
        long: Buf { data: "abcdefghij".repeat(7).into_bytes() },
    };
    debuggable_fixture_stop(&f);
}
