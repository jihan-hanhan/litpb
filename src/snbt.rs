//! Minimal SNBT (string NBT) serializer, byte-compatible with nbtlib's
//! `serialize_tag` default output (no indentation, non-compact).

pub enum Snbt {
    Int(i32),
    Str(String),
    IntArray(Vec<i32>),
    List(Vec<Snbt>),
    Compound(Vec<(String, Snbt)>),
}

impl Snbt {
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Snbt::Int(v) => out.push_str(&v.to_string()),
            Snbt::Str(s) => escape_string(s, out),
            Snbt::IntArray(values) => {
                out.push_str("[I; ");
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&v.to_string());
                }
                out.push(']');
            }
            Snbt::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Snbt::Compound(entries) => {
                out.push('{');
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if is_unquoted_key(key) {
                        out.push_str(key);
                    } else {
                        escape_string(key, out);
                    }
                    out.push_str(": ");
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// Mirrors nbtlib's UNQUOTED_COMPOUND_KEY regex: ^[a-zA-Z0-9._+-]+$
fn is_unquoted_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|b| {
            matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'_' | b'+' | b'-')
        })
}

/// Mirrors nbtlib's escape_string: quote defaults to `"`; if the string
/// contains a quote character, use the opposite quote of the first one found.
/// Always escape backslashes and the active quote character.
fn escape_string(s: &str, out: &mut String) {
    let quote = match s.find(|c| c == '"' || c == '\'') {
        Some(i) if s.as_bytes()[i] == b'"' => '\'',
        _ => '"',
    };
    out.push(quote);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
}
