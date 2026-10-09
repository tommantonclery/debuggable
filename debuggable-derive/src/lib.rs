//! Derive macro for [`debuggable`](https://docs.rs/debuggable). Use that crate instead of
//! depending on this one directly.
//!
//! The derive parses `#[debuggable(...)]` attributes, validates them, and expands to one
//! `::debuggable::__entry!` call carrying a JSON descriptor of the type
//! (see `docs/internal/schema-v1.md`). All rendering happens in the debugger.

mod emit;

use emit::{Alternative, Body, Field, Only, Part, Ty, Variant};
use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Error, Fields, GenericParam, LitStr, Result, Type};

/// Derives debugger visualizers for a struct or enum. See the `debuggable` crate docs.
#[proc_macro_derive(Debuggable, attributes(debuggable))]
pub fn derive_debuggable(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> Result<proc_macro2::TokenStream> {
    let ty = model(input)?;
    Ok(emit::entry_call(&ty).parse().expect("generated tokens are valid"))
}

// ---- Attribute options --------------------------------------------------------------------

/// Where an attribute appears; decides which options are allowed.
#[derive(Clone, Copy, PartialEq)]
enum Place {
    Struct,
    Enum,
    Variant,
    StructField,
    VariantField,
}

#[derive(Default)]
struct Opts {
    summary: Option<LitStr>,
    hide: Option<Span>,
    rename: Option<LitStr>,
    items: Option<Span>,
    text: Option<Span>,
    len: Option<LitStr>,
    only: Option<LitStr>,
    value: Option<LitStr>,
    /// On a type: one `(items, len)` per `#[debuggable(items = "...", ...)]` attribute.
    alternatives: Vec<(LitStr, Option<LitStr>)>,
}

fn opts(attrs: &[Attribute], place: Place) -> Result<Opts> {
    let mut o = Opts::default();
    let on_type = matches!(place, Place::Struct | Place::Enum);
    for attr in attrs.iter().filter(|a| a.path().is_ident("debuggable")) {
        // On a type, `items = "..."` and its `len` pair up within one attribute.
        let (mut alt_items, mut alt_len): (Option<LitStr>, Option<LitStr>) = (None, None);
        attr.parse_nested_meta(|m| {
            let key = m.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
            let span = m.path.span();
            let allowed = match key.as_str() {
                "summary" => matches!(place, Place::Struct | Place::Variant),
                "hide" | "rename" => matches!(place, Place::StructField | Place::VariantField),
                "items" | "len" if on_type => true,
                "items" | "text" | "len" | "only" | "value" => place == Place::StructField,
                _ => {
                    const OPTIONS: [&str; 8] = ["summary", "hide", "rename", "items", "text", "len", "only", "value"];
                    return Err(m.error(match closest(&key, OPTIONS) {
                        Some(best) => format!("unknown `debuggable` option `{key}`; did you mean `{best}`?"),
                        None => "unknown `debuggable` option; expected one of: `summary`, `hide`, `rename`, `items`, `text`, `len`, `only`, `value`"
                            .to_string(),
                    }));
                }
            };
            if !allowed {
                return Err(Error::new(span, misplaced(&key, place)));
            }
            let flag = |m: &syn::meta::ParseNestedMeta, set: &mut Option<Span>| -> Result<()> {
                if !m.input.is_empty() && !m.input.peek(syn::Token![,]) {
                    return Err(m.error(format!("`{key}` takes no value")));
                }
                if set.replace(span).is_some() {
                    return Err(Error::new(span, format!("duplicate `{key}`")));
                }
                Ok(())
            };
            let value = |m: &syn::meta::ParseNestedMeta, set: &mut Option<LitStr>| -> Result<()> {
                let lit: LitStr = m.value()?.parse().map_err(|_| m.error(format!("expected `{key} = \"...\"`")))?;
                if set.replace(lit).is_some() {
                    return Err(Error::new(span, format!("duplicate `{key}`")));
                }
                Ok(())
            };
            if on_type {
                let has_value = m.input.peek(syn::Token![=]);
                return match key.as_str() {
                    "items" if !has_value => Err(m.error("on a type, write `items = \"path\"` (one attribute per place the elements can be)")),
                    "items" => value(&m, &mut alt_items),
                    "len" => value(&m, &mut alt_len),
                    _ => value(&m, &mut o.summary),
                };
            }
            if key == "items" && m.input.peek(syn::Token![=]) {
                return Err(m.error("on a field, write `items` without a value (`items = \"path\"` goes on the type)"));
            }
            match key.as_str() {
                "summary" => value(&m, &mut o.summary),
                "rename" => value(&m, &mut o.rename),
                "len" => value(&m, &mut o.len),
                "only" => value(&m, &mut o.only),
                "value" => value(&m, &mut o.value),
                "hide" => flag(&m, &mut o.hide),
                "text" => flag(&m, &mut o.text),
                _ => flag(&m, &mut o.items),
            }
        })?;
        match (alt_items, alt_len) {
            (Some(items), len) => o.alternatives.push((items, len)),
            (None, Some(len)) => {
                return Err(Error::new(len.span(), "`len` on a type needs `items = \"...\"` in the same attribute"))
            }
            (None, None) => {}
        }
    }
    Ok(o)
}

fn misplaced(key: &str, place: Place) -> String {
    match (key, place) {
        ("summary", Place::Enum) => {
            "`summary` on an enum goes on each variant: `#[debuggable(summary = \"...\")]` above the variant".into()
        }
        ("summary", _) => "`summary` is allowed on a struct or an enum variant, not on a field".into(),
        ("items" | "text" | "len" | "only" | "value", Place::VariantField) => format!("`{key}` is not supported inside enum variants"),
        (_, Place::Struct | Place::Enum | Place::Variant) => format!("`{key}` is allowed on fields only"),
        _ => format!("`{key}` is not allowed here"),
    }
}

// ---- Model building and validation ---------------------------------------------------------

fn is_phantom(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.segments.last().is_some_and(|s| s.ident == "PhantomData"))
}

/// Fields plus their options, validated as a group.
fn fields(fields: &Fields, place: Place) -> Result<Vec<Field>> {
    let mut out = Vec::new();
    let mut items_seen = false;
    for (i, f) in fields.iter().enumerate() {
        let o = opts(&f.attrs, place)?;
        if let (Some(_), Some(rename)) = (o.hide, &o.rename) {
            return Err(Error::new(rename.span(), "a hidden field can't be renamed"));
        }
        if let (Some(_), Some(items)) = (o.hide, o.items) {
            return Err(Error::new(items, "`items` already replaces the field with its elements; remove `hide`"));
        }
        if let (Some(_), Some(text)) = (o.items, o.text) {
            return Err(Error::new(text, "a field can't have both `items` and `text`"));
        }
        for (lit, key) in [(&o.only, "only"), (&o.value, "value")] {
            if let (Some(lit), None) = (lit, o.items) {
                return Err(Error::new(lit.span(), format!("`{key}` needs `items` on the same field")));
            }
        }
        if let (Some(len), None, None) = (&o.len, o.items, o.text) {
            return Err(Error::new(len.span(), "`len` needs `items` or `text` on the same field"));
        }
        if let Some(items) = o.items {
            if items_seen {
                return Err(Error::new(items, "`items` may be used on only one field"));
            }
            items_seen = true;
        }
        if let Some(r) = &o.rename {
            if r.value().is_empty() {
                return Err(Error::new(r.span(), "`rename` needs a non-empty name"));
            }
        }
        out.push(Field {
            name: f.ident.as_ref().map(|id| id.unraw().to_string()).unwrap_or_else(|| i.to_string()),
            hide: o.hide.is_some() || is_phantom(&f.ty),
            rename: o.rename.map(|r| r.value()),
            items: o.items.is_some(),
            text: o.text.is_some(),
            only: o.only.as_ref().map(parse_only).transpose()?,
            value: o.value.as_ref().map(|v| parse_path(v, "value")).transpose()?,
            len: o.len.map(|l| (l.value(), l.span())),
        });
    }
    for f in &out {
        if let Some((len, span)) = &f.len {
            if !out.iter().any(|g| &g.name == len) {
                return Err(Error::new(*span, unknown_field(len, &out, "`len`")));
            }
        }
    }
    Ok(out)
}

/// `a.b.0`: field names (or tuple indices) separated by dots (design 0002 §2.1).
fn parse_path(lit: &LitStr, what: &str) -> Result<Vec<String>> {
    parse_path_str(&lit.value(), lit, what)
}

fn parse_path_str(s: &str, lit: &LitStr, what: &str) -> Result<Vec<String>> {
    let err = |msg: String| Error::new(lit.span(), msg);
    if s.trim().is_empty() {
        return Err(err(format!("`{what}` expects a field or variant name, like `\"value\"` or `\"u.value\"`")));
    }
    let mut out = Vec::new();
    for seg in s.split('.') {
        let seg = seg.trim();
        if seg.is_empty() {
            return Err(err(format!("empty segment in `{what}` path `{s}`")));
        }
        if let Some((first, rest)) = seg.split_once(char::is_whitespace) {
            let _ = first;
            return Err(err(format!("unexpected `{}` in `{what}`", rest.trim())));
        }
        let seg = seg.strip_prefix("r#").unwrap_or(seg);
        let is_ident = syn::parse::Parser::parse_str(syn::Ident::parse_any, seg).is_ok();
        if !(is_ident || seg.bytes().all(|b| b.is_ascii_digit())) {
            return Err(err(format!("`{seg}` in `{what}` is not a field name")));
        }
        out.push(seg.to_string());
    }
    Ok(out)
}

/// `path` or `path & mask` (design 0002 §2.1).
fn parse_only(lit: &LitStr) -> Result<Only> {
    let s = lit.value();
    let err = |msg: String| Error::new(lit.span(), msg);
    let (path, mask) = match s.split_once('&') {
        None => (s.as_str(), None),
        Some((path, mask)) => {
            let mask = mask.trim();
            if mask.is_empty() {
                return Err(err("`only` expects a mask after `&`, like `\"version & 1\"`".into()));
            }
            let parsed = match mask.strip_prefix("0x").or_else(|| mask.strip_prefix("0X")) {
                Some(hex) => u64::from_str_radix(&hex.replace('_', ""), 16),
                None => mask.replace('_', "").parse::<u64>(),
            };
            let n = parsed.map_err(|_| err(format!("`only` mask `{mask}` is not an integer")))?;
            if n == 0 {
                return Err(err("`only` mask must be non-zero".into()));
            }
            if n > (1 << 53) - 1 {
                return Err(err("`only` mask must be at most 2^53 - 1".into()));
            }
            (path, Some(n))
        }
    };
    if path.trim().is_empty() {
        return Err(err("`only` expects a field or variant name, optionally `& mask`: `\"version & 1\"`".into()));
    }
    Ok(Only { path: parse_path_str(path, lit, "only")?, mask })
}

/// The closest candidate for a "did you mean" hint. Like rustc, allow an edit distance of
/// at most a third of the name's length, and never suggest a name with nothing in common.
fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    /// Edit distance where swapping two adjacent characters counts as one edit, as in rustc
    /// (optimal string alignment).
    fn distance(a: &str, b: &str) -> usize {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut d: Vec<Vec<usize>> = (0..=a.len()).map(|i| vec![i; b.len() + 1]).collect();
        d[0] = (0..=b.len()).collect();
        for i in 1..=a.len() {
            for j in 1..=b.len() {
                let cost = usize::from(a[i - 1] != b[j - 1]);
                d[i][j] = (d[i - 1][j - 1] + cost).min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                    d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
                }
            }
        }
        d[a.len()][b.len()]
    }
    let limit = (name.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .map(|c| (distance(name, c), c))
        .filter(|&(d, _)| d <= limit && d < name.chars().count())
        .min()
        .map(|(_, c)| c)
}

fn unknown_field(name: &str, fields: &[Field], what: &str) -> String {
    if fields.is_empty() {
        return format!("{what} refers to `{name}`, but there are no fields");
    }
    if let Some(best) = closest(name, fields.iter().map(|f| f.name.as_str())) {
        return format!("{what} refers to unknown field `{name}`; did you mean `{best}`?");
    }
    let names: Vec<String> = fields.iter().map(|f| format!("`{}`", f.name)).collect();
    format!("{what} refers to unknown field `{name}`; available: {}", names.join(", "))
}

/// Parse a summary format string: `{field}` references, `{{`/`}}` escapes, literal text.
fn summary(lit: &LitStr, fields: &[Field], has_items: bool) -> Result<Vec<Part>> {
    let s = lit.value();
    let err = |msg: String| Error::new(lit.span(), msg);
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' if chars.peek() == Some(&'#') => {
                chars.next();
                if chars.next() != Some('}') {
                    return Err(err("`{#` must be followed by `}`: `{#}` is the number of elements".into()));
                }
                if !has_items {
                    return Err(err("`{#}` counts elements, but this type has no `items`".into()));
                }
                if !text.is_empty() {
                    parts.push(Part::Lit(std::mem::take(&mut text)));
                }
                parts.push(Part::Count);
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) if c.is_alphanumeric() || c == '_' => name.push(c),
                        Some(':') => {
                            return Err(err(format!(
                                "format specs are not supported in summaries: use `{{{name}}}`"
                            )))
                        }
                        Some(c) => {
                            return Err(err(format!(
                                "`{c}` in `{{{name}...}}`: summaries can only refer to fields by name, like `{{len}}` or `{{0}}`"
                            )))
                        }
                        None => return Err(err("unclosed `{` in summary (write `{{` for a literal brace)".into())),
                    }
                }
                if name.is_empty() {
                    return Err(err("empty `{}` in summary: name a field, like `{len}`".into()));
                }
                let name = name.strip_prefix("r#").map(str::to_string).unwrap_or(name);
                if !fields.iter().any(|f| f.name == name) {
                    return Err(err(unknown_field(&name, fields, "summary")));
                }
                if !text.is_empty() {
                    parts.push(Part::Lit(std::mem::take(&mut text)));
                }
                parts.push(Part::Field(name));
            }
            '}' => return Err(err("unmatched `}` in summary (write `}}` for a literal brace)".into())),
            c => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(Part::Lit(text));
    }
    Ok(parts)
}

fn model(input: &DeriveInput) -> Result<Ty> {
    let generic = input.generics.params.iter().any(|p| !matches!(p, GenericParam::Lifetime(_)));
    let (summary_lit, body, alts) = match &input.data {
        Data::Struct(s) => {
            let o = opts(&input.attrs, Place::Struct)?;
            let fs = fields(&s.fields, Place::StructField)?;
            let alts = alternatives(&o.alternatives)?;
            if let (Some((lit, _)), true) = (o.alternatives.first(), fs.iter().any(|f| f.items)) {
                return Err(Error::new(
                    lit.span(),
                    "use either field-level `items` or `items = \"...\"` on the type, not both",
                ));
            }
            for (alt, (lit, len)) in alts.iter().zip(&o.alternatives) {
                check_struct_path(&alt.items, lit, &fs, "`items`")?;
                if let (Some(path), Some(len)) = (&alt.len, len) {
                    check_struct_path(path, len, &fs, "`len`")?;
                }
            }
            (o.summary, Body::Struct(fs), alts)
        }
        Data::Enum(e) => {
            let o = opts(&input.attrs, Place::Enum)?;
            let alts = alternatives(&o.alternatives)?;
            let mut variants = Vec::new();
            for v in &e.variants {
                let o = opts(&v.attrs, Place::Variant)?;
                let fs = fields(&v.fields, Place::VariantField)?;
                let summary = o.summary.as_ref().map(|lit| summary(lit, &fs, !alts.is_empty())).transpose()?;
                variants.push(Variant { name: v.ident.unraw().to_string(), summary, fields: fs });
            }
            for (alt, (lit, len)) in alts.iter().zip(&o.alternatives) {
                check_enum_path(&alt.items, lit, &variants)?;
                if let (Some(path), Some(len)) = (&alt.len, len) {
                    check_enum_path(path, len, &variants)?;
                }
            }
            (None, Body::Enum(variants), alts)
        }
        Data::Union(u) => {
            return Err(Error::new(u.union_token.span, "`Debuggable` can't be derived for unions"));
        }
    };
    let summary = match (&summary_lit, &body) {
        (Some(lit), Body::Struct(fs)) => Some(summary(lit, fs, !alts.is_empty() || fs.iter().any(|f| f.items))?),
        _ => None,
    };
    Ok(Ty { name: input.ident.unraw().to_string(), generic, summary, body, alternatives: alts })
}

/// Parse the paths of type-level `items = "..."` alternatives (design 0003).
fn alternatives(raw: &[(LitStr, Option<LitStr>)]) -> Result<Vec<Alternative>> {
    raw.iter()
        .map(|(items, len)| {
            Ok(Alternative {
                items: parse_path(items, "items")?,
                len: len.as_ref().map(|l| parse_path(l, "len")).transpose()?,
            })
        })
        .collect()
}

/// On a struct, a path must start at one of its fields.
fn check_struct_path(path: &[String], lit: &LitStr, fields: &[Field], what: &str) -> Result<()> {
    if fields.iter().any(|f| f.name == path[0]) {
        return Ok(());
    }
    Err(Error::new(lit.span(), unknown_field(&path[0], fields, what)))
}

/// On an enum, a path starts at a variant, then (if it goes on) one of that variant's fields.
fn check_enum_path(path: &[String], lit: &LitStr, variants: &[Variant]) -> Result<()> {
    let Some(v) = variants.iter().find(|v| v.name == path[0]) else {
        let msg = match closest(&path[0], variants.iter().map(|v| v.name.as_str())) {
            Some(best) => format!("unknown variant `{}`; did you mean `{best}`?", path[0]),
            None => {
                let names: Vec<String> = variants.iter().map(|v| format!("`{}`", v.name)).collect();
                format!("unknown variant `{}`; the enum has {}", path[0], names.join(", "))
            }
        };
        return Err(Error::new(lit.span(), msg));
    };
    match path.get(1) {
        Some(field) if !v.fields.iter().any(|f| &f.name == field) => {
            let mut msg = format!("variant `{}` has no field `{field}`", v.name);
            if let Some(best) = closest(field, v.fields.iter().map(|f| f.name.as_str())) {
                msg.push_str(&format!("; did you mean `{best}`?"));
            }
            Err(Error::new(lit.span(), msg))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(src: &str) -> String {
        let input: DeriveInput = syn::parse_str(src).unwrap();
        let call = emit::entry_call(&model(&input).unwrap());
        // `::debuggable::__entry!("Name", "<json tail>");` -> the JSON tail, unescaped
        let lit: LitStr = syn::parse_str(call.rsplit_once(", ").unwrap().1.trim_end_matches(");")).unwrap();
        lit.value()
    }

    fn error(src: &str) -> String {
        let input: DeriveInput = syn::parse_str(src).unwrap();
        match model(&input) {
            Ok(_) => panic!("expected an error for: {src}"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn struct_with_everything() {
        assert_eq!(
            json(r#"#[debuggable(summary = "{len} items")] struct M<K, V> {
                #[debuggable(items, len = "len")] slots: Vec<V>,
                #[debuggable(hide)] free_head: u32,
                len: u32,
                _k: std::marker::PhantomData<K>,
                #[debuggable(rename = "count")] n: u8,
            }"#),
            r#""generic":true,"kind":"struct","summary":[["field","len"],["lit"," items"]],"hide":["free_head","_k"],"rename":{"n":"count"},"items":{"field":"slots","len":"len"}}"#
        );
    }

    #[test]
    fn text_fields() {
        assert_eq!(
            json(r#"#[debuggable(summary = "{xs}")] struct S<const N: usize> {
                #[debuggable(hide)] len: u8,
                #[debuggable(text, len = "len", hide)] xs: [u8; N],
                #[debuggable(text)] tag: [u8; 4],
            }"#),
            r#""generic":true,"kind":"struct","summary":[["field","xs"]],"hide":["len","xs"],"text":[{"field":"xs","len":"len"},{"field":"tag"}]}"#
        );
    }

    #[test]
    fn slots_fields() {
        assert_eq!(
            json(r#"#[debuggable(summary = "{len} items")] struct S<T> {
                #[debuggable(items, len = "n", only = "version & 0x1", value = "u.r#value")] slots: Vec<T>,
                n: usize, len: u32,
            }"#),
            r#""generic":true,"kind":"struct","summary":[["field","len"],["lit"," items"]],"slots":{"field":"slots","len":"n","only":{"path":["version"],"mask":1},"value":["u","value"]}}"#
        );
        assert_eq!(
            json(r#"struct S { #[debuggable(items, only = "Occupied", value = "0")] e: Vec<u8> }"#),
            r#""generic":false,"kind":"struct","slots":{"field":"e","only":{"path":["Occupied"]},"value":["0"]}}"#
        );
        assert_eq!(
            // plain items: unchanged
            json(r#"struct S { #[debuggable(items)] e: Vec<u8> }"#),
            r#""generic":false,"kind":"struct","items":{"field":"e"}}"#
        );
    }

    #[test]
    fn alternatives_on_struct_and_enum() {
        assert_eq!(
            json(r#"#[debuggable(summary = "{#} items")]
                #[debuggable(items = "data.Inline.0", len = "capacity")]
                #[debuggable(items = "data.Heap.ptr", len = "data.Heap.len")]
                struct SmallVec<A> { #[debuggable(hide)] capacity: usize, #[debuggable(hide)] data: D<A> }"#),
            r#""generic":true,"kind":"struct","summary":[["count",""],["lit"," items"]],"hide":["capacity","data"],"alternatives":[{"items":["data","Inline","0"],"len":["capacity"]},{"items":["data","Heap","ptr"],"len":["data","Heap","len"]}]}"#
        );
        assert_eq!(
            json(r#"#[debuggable(items = "Inline.0.data", len = "Inline.0.len")]
                #[debuggable(items = "Heap.0")]
                enum TinyVec<A> { #[debuggable(summary = "{#} items")] Inline(#[debuggable(hide)] AV<A>), Heap(#[debuggable(hide)] Vec<A>) }"#),
            r#""generic":true,"kind":"enum","variants":{"Inline":{"summary":[["count",""],["lit"," items"]],"hide":["0"]},"Heap":{"hide":["0"]}},"alternatives":[{"items":["Inline","0","data"],"len":["Inline","0","len"]},{"items":["Heap","0"]}]}"#
        );
        assert_eq!(
            // `{#}` with field-level items too
            json(r#"#[debuggable(summary = "{#}")] struct S { #[debuggable(items)] v: Vec<u8> }"#),
            r#""generic":false,"kind":"struct","summary":[["count",""]],"items":{"field":"v"}}"#
        );
    }

    #[test]
    fn enum_variants_and_defaults() {
        assert_eq!(
            json(r#"enum Token { #[debuggable(summary = "Ident({name})")] Ident { name: String }, #[debuggable(summary = "Num({0})")] Num(i64), Eof }"#),
            r#""generic":false,"kind":"enum","variants":{"Ident":{"summary":[["lit","Ident("],["field","name"],["lit",")"]]},"Num":{"summary":[["lit","Num("],["field","0"],["lit",")"]]}}}"#
        );
        assert_eq!(json("enum E { A, B }"), r#""generic":false,"kind":"enum","variants":{}}"#);
    }

    #[test]
    fn lifetimes_alone_are_not_generic_and_raw_idents_are_unraw() {
        assert_eq!(
            json(r#"#[debuggable(summary = "{type}")] struct r#S<'a> { r#type: &'a str }"#),
            r#""generic":false,"kind":"struct","summary":[["field","type"]]}"#
        );
    }

    #[test]
    fn json_is_ascii_and_quote_safe() {
        // `bs` is a backslash, so the expected JSON escapes stay visible as escapes.
        let bs = '\\';
        assert_eq!(
            json(r#"#[debuggable(summary = "{0}°C ''' \"q\" {{x}}")] struct C(f32);"#),
            format!(r#""generic":false,"kind":"struct","summary":[["field","0"],["lit","{bs}u00b0C {bs}u0027{bs}u0027{bs}u0027 {bs}"q{bs}" {{x}}"]]}}"#)
        );
    }

    #[test]
    fn errors() {
        let cases = [
            (r#"#[debuggable(sumary = "x")] struct S;"#, "unknown `debuggable` option `sumary`; did you mean `summary`?"),
            (r#"#[debuggable(colour = "x")] struct S;"#, "expected one of: `summary`"),
            (r#"#[debuggable(summary = "{lenght}")] struct S { length: u8 }"#, "did you mean `length`?"),
            (r#"#[debuggable(summary = "a", summary = "b")] struct S;"#, "duplicate `summary`"),
            (r#"#[debuggable(summary = "x")] enum E { A }"#, "goes on each variant"),
            (r#"#[debuggable(hide)] struct S { a: u8 }"#, "fields only"),
            (r#"struct S { #[debuggable(summary = "x")] a: u8 }"#, "not on a field"),
            (r#"enum E { A { #[debuggable(items)] v: Vec<u8> } }"#, "not supported inside enum variants"),
            (r#"struct S { #[debuggable(len = "n")] v: Vec<u8>, n: usize }"#, "`len` needs `items` or `text`"),
            (r#"struct S { #[debuggable(items, text)] v: Vec<u8> }"#, "both `items` and `text`"),
            (r#"struct S { #[debuggable(text, text)] v: Vec<u8> }"#, "duplicate `text`"),
            (r#"struct S { #[debuggable(txt)] v: Vec<u8> }"#, "did you mean `text`?"),
            (r#"enum E { A { #[debuggable(text)] v: Vec<u8> } }"#, "`text` is not supported inside enum variants"),
            (r#"struct S { #[debuggable(items)] a: Vec<u8>, #[debuggable(items)] b: Vec<u8> }"#, "only one field"),
            (r#"struct S { #[debuggable(items, len = "count")] v: Vec<u8>, n: usize }"#, "unknown field `count`; available: `v`, `n`"),
            (r#"#[debuggable(summary = "{x}")] struct S { a: u8 }"#, "unknown field `x`; available: `a`"),
            (r#"#[debuggable(summary = "{0}")] struct S { a: u8 }"#, "unknown field `0`"),
            (r#"#[debuggable(summary = "{a:?}")] struct S { a: u8 }"#, "format specs are not supported"),
            (r#"#[debuggable(summary = "{a.b}")] struct S { a: u8 }"#, "can only refer to fields by name"),
            (r#"#[debuggable(summary = "{a")] struct S { a: u8 }"#, "unclosed `{`"),
            (r#"#[debuggable(summary = "a}")] struct S { a: u8 }"#, "unmatched `}`"),
            (r#"#[debuggable(summary = "{}")] struct S { a: u8 }"#, "empty `{}`"),
            (r#"struct S { #[debuggable(hide, rename = "b")] a: u8 }"#, "hidden field can't be renamed"),
            (r#"struct S { #[debuggable(hide, items)] a: Vec<u8> }"#, "remove `hide`"),
            (r#"struct S { #[debuggable(hide = "yes")] a: u8 }"#, "takes no value"),
            (r#"struct S { #[debuggable(rename = 3)] a: u8 }"#, "expected `rename = \"...\"`"),
            (r#"struct S { #[debuggable(rename = "")] a: u8 }"#, "non-empty"),
            (r#"union U { a: u8 }"#, "can't be derived for unions"),
            (r#"struct S { #[debuggable(only = "x")] v: Vec<u8> }"#, "`only` needs `items`"),
            (r#"struct S { #[debuggable(value = "x")] v: Vec<u8> }"#, "`value` needs `items`"),
            (r#"struct S { #[debuggable(items, only = "")] v: Vec<u8> }"#, "`only` expects a field or variant name"),
            (r#"struct S { #[debuggable(items, only = "a..b")] v: Vec<u8> }"#, "empty segment"),
            (r#"struct S { #[debuggable(items, only = "a & 0")] v: Vec<u8> }"#, "mask must be non-zero"),
            (r#"struct S { #[debuggable(items, only = "a & 9007199254740992")] v: Vec<u8> }"#, "at most 2^53 - 1"),
            (r#"struct S { #[debuggable(items, only = "a &")] v: Vec<u8> }"#, "expects a mask"),
            (r#"struct S { #[debuggable(items, only = "a b")] v: Vec<u8> }"#, "unexpected `b`"),
            (r#"struct S { #[debuggable(items, value = "a.-b")] v: Vec<u8> }"#, "not a field name"),
            (r#"struct S { #[debuggable(items, only = "a", only = "b")] v: Vec<u8> }"#, "duplicate `only`"),
            (r#"struct S { #[debuggable(items, onyl = "x")] v: Vec<u8> }"#, "did you mean `only`?"),
            (r#"#[debuggable(items = "dta.x")] struct S { data: u8 }"#, "`items` refers to unknown field `dta`; did you mean `data`?"),
            (r#"#[debuggable(items = "Hep.0")] enum E { Heap(Vec<u8>) }"#, "unknown variant `Hep`; did you mean `Heap`?"),
            (r#"#[debuggable(items = "Heap.x")] enum E { Heap(Vec<u8>) }"#, "variant `Heap` has no field `x`"),
            (r#"#[debuggable(len = "n")] struct S { n: u8 }"#, "`len` on a type needs `items = \"...\"` in the same attribute"),
            (r#"struct S { #[debuggable(items = "v")] v: Vec<u8> }"#, "on a field, write `items` without a value"),
            (r#"#[debuggable(items = "v")] struct S { #[debuggable(items)] v: Vec<u8> }"#, "either field-level `items` or `items = \"...\"` on the type, not both"),
            (r#"#[debuggable(summary = "{#}")] struct S { a: u8 }"#, "`{#}` counts elements, but this type has no `items`"),
            (r#"#[debuggable(items = "v")] struct S { #[debuggable(only = "x")] v: Vec<u8> }"#, "`only` needs `items`"),
            (r#"#[debuggable(items, len = "n")] struct S { n: u8 }"#, "on a type, write `items = \"path\"`"),
        ];
        for (src, want) in cases {
            let got = error(src);
            assert!(got.contains(want), "for {src}\n  want: {want}\n  got:  {got}");
        }
    }
}
