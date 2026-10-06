//! Enums: tagged and niche layouts (64-bit and char niches), arrays (GDB's unresolved
//! variants), references, per-variant summaries and the variant-name default.
#![allow(dead_code)]
use debuggable::Debuggable;

#[derive(Debuggable)]
pub enum Token {
    #[debuggable(summary = "Ident({name})")]
    Ident { name: String },
    #[debuggable(summary = "Num({0})")]
    Num(i64),
    Eof,
}

// niche in a char (fits in 32 bits); no summaries at all
#[derive(Debuggable)]
pub enum Glyph { Char(char), Space, Newline }

// explicit tag, hidden field in one variant
#[derive(Debuggable)]
pub enum Tagged {
    X(u64),
    Y { keep: u64, #[debuggable(hide)] drop: u64 },
    Z,
}

pub struct Fixture<'a> {
    pub ident: Token,
    pub num: Token,
    pub eof: Token,
    pub tokens: [Token; 3],
    pub glyphs: [Glyph; 3],
    pub tagged: [Tagged; 3],
    pub by_ref: &'a Token,
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
    let shared = Token::Num(-1);
    let f = Fixture {
        ident: Token::Ident { name: "foo".into() },
        num: Token::Num(42),
        eof: Token::Eof,
        tokens: [Token::Ident { name: "bar".into() }, Token::Num(7), Token::Eof],
        glyphs: [Glyph::Char('q'), Glyph::Space, Glyph::Newline],
        tagged: [Tagged::X(1), Tagged::Y { keep: 2, drop: 3 }, Tagged::Z],
        by_ref: &shared,
    };
    debuggable_fixture_stop(&f);
}
