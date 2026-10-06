//! Internals used by `#[derive(Debuggable)]` expansions. Not public API: anything here
//! can change in any release. See `docs/internal/schema-v1.md`.

/// One link-section entry: exactly the bytes GDB / the LLDB loader read.
/// `repr(transparent)` so the static's contents are the entry and nothing else.
#[repr(transparent)]
pub struct Entry<const N: usize>(pub [u8; N]);

/// Total length of `parts` once concatenated. Used for the `Entry<N>` length.
pub const fn total_len(parts: &[&[u8]]) -> usize {
    let mut n = 0;
    let mut p = 0;
    while p < parts.len() {
        n += parts[p].len();
        p += 1;
    }
    n
}

impl<const N: usize> Entry<N> {
    /// Concatenate `parts` at compile time. Fails const evaluation if the length is wrong.
    pub const fn new(parts: &[&[u8]]) -> Self {
        let mut out = [0u8; N];
        let mut i = 0;
        let mut p = 0;
        while p < parts.len() {
            let mut j = 0;
            while j < parts[p].len() {
                out[i] = parts[p][j];
                i += 1;
                j += 1;
            }
            p += 1;
        }
        assert!(i == N, "debuggable: entry length mismatch");
        Entry(out)
    }
}

/// Emits one descriptor entry (schema-v1 §3.1). Called by the derive as:
///
/// ```ignore
/// ::debuggable::__entry!("SlotMap", "\"generic\":true,\"kind\":\"struct\"}");
/// ```
///
/// The first argument is the type's identifier. The second is the rest of the descriptor JSON
/// after `"path"`, already escaped per §3.1 and ending in `}`.
#[macro_export]
#[doc(hidden)]
macro_rules! __entry {
    ($ident:literal, $json_tail:literal) => {
        const _: () = {
            const PARTS: &[&[u8]] = &[
                b"\x04debuggable-v1-",
                ::core::concat!(::core::module_path!(), "::", $ident, "@", ::core::env!("CARGO_PKG_VERSION"))
                    .as_bytes(),
                b"\nimport gdb\ngdb.__dict__.setdefault('_debuggable_q',[]).append((gdb.current_objfile(),r'''{\"v\":1,\"path\":\"",
                ::core::concat!(::core::module_path!(), "::", $ident).as_bytes(),
                b"\",",
                $json_tail.as_bytes(),
                b"'''))\ngetattr(gdb,'_debuggable_flush',lambda:None)()\n\0",
            ];
            $crate::__entry_static!(
                $crate::__private::Entry<{ $crate::__private::total_len(PARTS) }>,
                $crate::__private::Entry::new(PARTS)
            );
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
    use super::{total_len, Entry};

    // Name must match MINOR in runtime/gdb.py; tools/gen-runtime.py --check enforces it.
    // Runs in a private namespace so nothing leaks into GDB's shared __main__.
    const PARTS: &[&[u8]] = &[
        b"\x04debuggable-runtime-gdb-v1.3\nimport zlib,base64;exec(zlib.decompress(base64.b64decode('",
        include_bytes!("runtime/gdb.py.zb64"),
        b"')),{'__name__':'debuggable_runtime'})\n\0",
    ];

    #[used]
    #[allow(unsafe_code)] // `link_section` only; the contents are inert bytes read by debuggers
    #[link_section = ".debug_gdb_scripts"]
    static RUNTIME: Entry<{ total_len(PARTS) }> = Entry::new(PARTS);
}
