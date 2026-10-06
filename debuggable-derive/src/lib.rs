//! Derive macro for [`debuggable`](https://docs.rs/debuggable). Use that crate instead of
//! depending on this one directly.
//!
//! The derive parses `#[debuggable(...)]` attributes, validates them, and expands to one
//! `::debuggable::__entry!` call carrying a JSON descriptor of the type
//! (see `docs/internal/schema-v1.md`). All rendering happens in the debugger.

mod emit;

use emit::{Body, Field, Part, Ty, Variant};
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
    len: Option<LitStr>,
}

fn opts(attrs: &[Attribute], place: Place) -> Result<Opts> {
    let mut o = Opts::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("debuggable")) {
        attr.parse_nested_meta(|m| {
            let key = m.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
            let span = m.path.span();
            let allowed = match key.as_str() {
                "summary" => matches!(place, Place::Struct | Place::Variant),
                "hide" | "rename" => matches!(place, Place::StructField | Place::VariantField),
                "items" | "len" => place == Place::StructField,
                _ => {
                    const OPTIONS: [&str; 5] = ["summary", "hide", "rename", "items", "len"];
                    return Err(m.error(match closest(&key, OPTIONS) {
                        Some(best) => format!("unknown `debuggable` option `{key}`; did you mean `{best}`?"),
                        None => "unknown `debuggable` option; expected one of: `summary`, `hide`, `rename`, `items`, `len`"
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
            match key.as_str() {
                "summary" => value(&m, &mut o.summary),
                "rename" => value(&m, &mut o.rename),
                "len" => value(&m, &mut o.len),
                "hide" => flag(&m, &mut o.hide),
                _ => flag(&m, &mut o.items),
            }
        })?;
    }
    Ok(o)
}

fn misplaced(key: &str, place: Place) -> String {
    match (key, place) {
        ("summary", Place::Enum) => {
            "`summary` on an enum goes on each variant: `#[debuggable(summary = \"...\")]` above the variant".into()
        }
        ("summary", _) => "`summary` is allowed on a struct or an enum variant, not on a field".into(),
        ("items" | "len", Place::VariantField) => format!("`{key}` is not supported inside enum variants"),
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
        if let (Some(len), None) = (&o.len, o.items) {
            return Err(Error::new(len.span(), "`len` needs `items` on the same field"));
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

/// The closest candidate for a "did you mean" hint. Like rustc, allow an edit distance of
/// at most a third of the name's length, and never suggest a name with nothing in common.
fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    fn distance(a: &str, b: &str) -> usize {
        let b: Vec<char> = b.chars().collect();
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.chars().enumerate() {
            let mut prev = row[0];
            row[0] = i + 1;
            for j in 0..b.len() {
                let cur = row[j + 1];
                row[j + 1] = (prev + usize::from(ca != b[j])).min(row[j] + 1).min(cur + 1);
                prev = cur;
            }
        }
        row[b.len()]
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
fn summary(lit: &LitStr, fields: &[Field]) -> Result<Vec<Part>> {
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
    let (summary_lit, body) = match &input.data {
        Data::Struct(s) => {
            let o = opts(&input.attrs, Place::Struct)?;
            let fs = fields(&s.fields, Place::StructField)?;
            (o.summary, Body::Struct(fs))
        }
        Data::Enum(e) => {
            opts(&input.attrs, Place::Enum)?;
            let mut variants = Vec::new();
            for v in &e.variants {
                let o = opts(&v.attrs, Place::Variant)?;
                let fs = fields(&v.fields, Place::VariantField)?;
                let summary = o.summary.as_ref().map(|lit| summary(lit, &fs)).transpose()?;
                variants.push(Variant { name: v.ident.unraw().to_string(), summary, fields: fs });
            }
            (None, Body::Enum(variants))
        }
        Data::Union(u) => {
            return Err(Error::new(u.union_token.span, "`Debuggable` can't be derived for unions"));
        }
    };
    let summary = match (&summary_lit, &body) {
        (Some(lit), Body::Struct(fs)) => Some(summary(lit, fs)?),
        _ => None,
    };
    Ok(Ty { name: input.ident.unraw().to_string(), generic, summary, body })
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
            (r#"struct S { #[debuggable(len = "n")] v: Vec<u8>, n: usize }"#, "needs `items`"),
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
        ];
        for (src, want) in cases {
            let got = error(src);
            assert!(got.contains(want), "for {src}\n  want: {want}\n  got:  {got}");
        }
    }
}
