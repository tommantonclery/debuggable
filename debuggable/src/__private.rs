//! Internals used by `#[derive(Debuggable)]` expansions. Not public API: anything here
//! can change in any release. See `docs/internal/schema-v1.md`.

/// `s` as a byte array, for a link-section static's initializer.
///
/// Entries are assembled with `concat!`, which costs nothing at const evaluation (it is done
/// during macro expansion); this conversion is the only step left for the const evaluator.
/// A byte-by-byte copy there was measured at about 4.5 ms per derived type without
/// incremental compilation; this is a single read. See `tools/bench-compile-time.py`.
pub const fn bytes<const N: usize>(s: &str) -> [u8; N] {
    assert!(s.len() == N, "debuggable: entry length mismatch");
    // SAFETY: `s` is valid for reads of `s.len() == N` bytes (checked above), and `[u8; N]`
    // has alignment 1 with every bit pattern valid.
    #[allow(unsafe_code)]
    unsafe {
        *(s.as_ptr() as *const [u8; N])
    }
}

/// Emits one descriptor entry (schema-v1 §3.1). Called by the derive as:
///
/// ```ignore
/// ::debuggable::__entry!("SlotMap", "\"generic\":true,\"kind\":\"struct\"}");
/// ```
///
/// The first argument is the type's identifier. The second is the rest of the descriptor JSON
/// after `"path"`, already escaped per §3.1 and ending in `}`. Every piece is ASCII text, so
/// the whole entry is one `concat!`.
#[macro_export]
#[doc(hidden)]
macro_rules! __entry {
    ($ident:literal, $json_tail:literal) => {
        const _: () = {
            const TEXT: &str = ::core::concat!(
                "\x04debuggable-v1-",
                ::core::module_path!(),
                "::",
                $ident,
                "@",
                ::core::env!("CARGO_PKG_VERSION"),
                "\nimport gdb\ngdb.__dict__.setdefault('_debuggable_q',[]).append((gdb.current_objfile(),r'''{\"v\":1,\"path\":\"",
                ::core::module_path!(),
                "::",
                $ident,
                "\",",
                $json_tail,
                "'''))\ngetattr(gdb,'_debuggable_flush',lambda:None)()\n\0",
            );
            $crate::__entry_static!([u8; TEXT.len()], $crate::__private::bytes(TEXT));
        };
    };
}

// ---- Per-target section selection (schema-v1 §2) -------------------------------------
// Evaluated when *this* crate compiles, so the user's crate never sees our cfgs (§2.1).
// Exactly one of the four definitions below is active.

/// ELF (Linux, BSDs, Android, illumos, ...): GDB auto-loads it, and the LLDB loader reads it.
#[cfg(all(not(debuggable_disable), unix, not(target_vendor = "apple"), not(target_os = "aix")))]
#[macro_export]
#[doc(hidden)]
macro_rules! __entry_static {
    ($ty:ty, $init:expr) => {
        $crate::__entry_static_in!(".debug_gdb_scripts", $ty, $init);
    };
}

/// Mach-O: `.debug_gdb_scripts` is an invalid section specifier here; read by the LLDB loader.
#[cfg(all(not(debuggable_disable), target_vendor = "apple"))]
#[macro_export]
#[doc(hidden)]
macro_rules! __entry_static {
    ($ty:ty, $init:expr) => {
        $crate::__entry_static_in!("__DATA,__debuggable", $ty, $init);
    };
}

/// Disabled via `--cfg debuggable_disable`, or a target with no consumer in v1
/// (Windows, wasm, AIX, ...): emit nothing.
#[cfg(any(
    debuggable_disable,
    not(any(all(unix, not(target_os = "aix")), target_vendor = "apple"))
))]
#[macro_export]
#[doc(hidden)]
macro_rules! __entry_static {
    ($ty:ty, $init:expr) => {};
}

/// The one place an entry static is written. It lives in this crate's `macro_rules!`, so the
/// attribute tokens carry this crate's edition (2021) and rustc does not apply the user's
/// `unsafe_code` lint level to them. Never add `#[allow(unsafe_code)]` here: under a user's
/// `#![forbid(unsafe_code)]` that is a hard error (E0453).
#[macro_export]
#[doc(hidden)]
macro_rules! __entry_static_in {
    ($section:literal, $ty:ty, $init:expr) => {
        #[used]
        #[link_section = $section]
        static ENTRY: $ty = $init;
    };
}

// ---- The shared GDB runtime entry (schema-v1 §3.2) -------------------------------------
// One per facade version in the final binary; GDB has no consumer elsewhere.

#[cfg(all(not(debuggable_disable), unix, not(target_vendor = "apple"), not(target_os = "aix")))]
mod gdb_runtime {
    // Name must match MINOR in runtime/gdb.py; tools/gen-runtime.py --check enforces it.
    // Runs in a private namespace so nothing leaks into GDB's shared __main__.
    const TEXT: &str = concat!(
        "\x04debuggable-runtime-gdb-v1.5\nimport zlib,base64;exec(zlib.decompress(base64.b64decode('",
        include_str!("runtime/gdb.py.zb64"),
        "')),{'__name__':'debuggable_runtime'})\n\0",
    );

    #[used]
    #[allow(unsafe_code)] // `link_section` only; the contents are inert bytes read by debuggers
    #[link_section = ".debug_gdb_scripts"]
    static RUNTIME: [u8; TEXT.len()] = super::bytes(TEXT);
}
