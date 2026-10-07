//! Validated model -> `::debuggable::__entry!(...)` call text (schema-v1 §3.1, §4).
//! Everything here is infallible: validation happens while building the model.

use proc_macro2::Span;

pub(crate) enum Part {
    Lit(String),
    Field(String),
}

pub(crate) struct Field {
    /// Source name without `r#`; tuple fields are "0", "1", ...
    pub name: String,
    /// `#[debuggable(hide)]`, or a `PhantomData` field (hidden automatically).
    pub hide: bool,
    pub rename: Option<String>,
    pub items: bool,
    /// `#[debuggable(text)]`: shown as a string (with `len`, at most that many bytes).
    pub text: bool,
    pub len: Option<(String, Span)>,
}

pub(crate) struct Variant {
    pub name: String,
    pub summary: Option<Vec<Part>>,
    pub fields: Vec<Field>,
}

pub(crate) enum Body {
    Struct(Vec<Field>),
    Enum(Vec<Variant>),
}

pub(crate) struct Ty {
    pub name: String,
    pub generic: bool,
    pub summary: Option<Vec<Part>>,
    pub body: Body,
}

/// A JSON string literal that is ASCII-only and never contains `'` (schema-v1 §3.1).
fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' if c != '\'' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn list<T>(items: &[T], out: &mut String, mut each: impl FnMut(&T, &mut String)) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        each(item, out);
    }
    out.push(']');
}

/// `,"summary":...,"hide":...,"rename":...,"items":...,"text":...` for one field set (may be empty).
fn members(summary: Option<&[Part]>, fields: &[Field], out: &mut String) {
    if let Some(parts) = summary {
        out.push_str(",\"summary\":");
        list(parts, out, |p, out| {
            let (kind, text) = match p {
                Part::Lit(t) => ("lit", t),
                Part::Field(f) => ("field", f),
            };
            out.push_str("[\"");
            out.push_str(kind);
            out.push_str("\",");
            json_str(text, out);
            out.push(']');
        });
    }
    let hidden: Vec<&Field> = fields.iter().filter(|f| f.hide).collect();
    if !hidden.is_empty() {
        out.push_str(",\"hide\":");
        list(&hidden, out, |f, out| json_str(&f.name, out));
    }
    let renamed: Vec<&Field> = fields.iter().filter(|f| f.rename.is_some()).collect();
    if !renamed.is_empty() {
        out.push_str(",\"rename\":{");
        for (i, f) in renamed.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_str(&f.name, out);
            out.push(':');
            json_str(f.rename.as_deref().unwrap_or_default(), out);
        }
        out.push('}');
    }
    if let Some(f) = fields.iter().find(|f| f.items) {
        out.push_str(",\"items\":");
        source(f, out);
    }
    let texts: Vec<&Field> = fields.iter().filter(|f| f.text).collect();
    if !texts.is_empty() {
        out.push_str(",\"text\":");
        list(&texts, out, |f, out| source(f, out));
    }
}

/// `{"field":"xs","len":"len"}`: a field and its optional length field.
fn source(f: &Field, out: &mut String) {
    out.push_str("{\"field\":");
    json_str(&f.name, out);
    if let Some((len, _)) = &f.len {
        out.push_str(",\"len\":");
        json_str(len, out);
    }
    out.push('}');
}

/// The descriptor JSON after `"path"` (the facade's `__entry!` adds `{"v":1,"path":...,`).
pub(crate) fn json_tail(ty: &Ty) -> String {
    let mut j = String::from(if ty.generic { "\"generic\":true" } else { "\"generic\":false" });
    match &ty.body {
        Body::Struct(fields) => {
            j.push_str(",\"kind\":\"struct\"");
            members(ty.summary.as_deref(), fields, &mut j);
        }
        Body::Enum(variants) => {
            j.push_str(",\"kind\":\"enum\",\"variants\":{");
            let mut first = true;
            for v in variants {
                let mut m = String::new();
                members(v.summary.as_deref(), &v.fields, &mut m);
                if m.is_empty() {
                    continue; // default rendering: no entry needed (schema-v1 §4)
                }
                if !first {
                    j.push(',');
                }
                first = false;
                json_str(&v.name, &mut j);
                j.push_str(":{");
                j.push_str(&m[1..]);
                j.push('}');
            }
            j.push('}');
        }
    }
    j.push('}');
    j
}

/// `::debuggable::__entry!("Name", "<json tail>");`
pub(crate) fn entry_call(ty: &Ty) -> String {
    format!("::debuggable::__entry!({:?}, {:?});", ty.name, json_tail(ty))
}
