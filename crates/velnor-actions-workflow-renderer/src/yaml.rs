//! Deterministic YAML emitter: stable order, safe quoting, 2-space indent.
//!
//! Block style only, no anchors, aliases, or tags. Flow sequences are
//! empty `[]` plus the typed `runs-on` selector. Key order is caller-controlled.

/// Minimal YAML value tree with explicit mapping order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Yaml {
    /// Null renders as a bare key (`key:`).
    Null,
    /// String scalar, quoted only when plain style is unsafe.
    Str(String),
    /// Boolean scalar.
    Bool(bool),
    /// Integer scalar.
    Int(i64),
    /// Block sequence.
    Seq(Vec<Self>),
    /// Block mapping in the given entry order.
    Map(Vec<(String, Self)>),
    /// One-line flow sequence of scalars: `[a, b]`.
    Flow(Vec<String>),
    /// Double-quoted scalar. Plain `6379:6379` is a mapping in a sequence.
    Quoted(String),
    /// Plain or quoted scalar plus a trailing YAML comment.
    Annotated {
        /// Scalar text.
        value: String,
        /// Comment text, without the leading `#`.
        comment: String,
    },
}

impl Yaml {
    /// Build a string scalar.
    #[must_use]
    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    /// Build a scalar that is always double-quoted.
    #[must_use]
    pub fn quoted(value: impl Into<String>) -> Self {
        Self::Quoted(value.into())
    }

    /// Build a scalar with a trailing comment on the same line.
    #[must_use]
    pub fn annotated(value: impl Into<String>, comment: impl Into<String>) -> Self {
        Self::Annotated {
            value: value.into(),
            comment: comment.into(),
        }
    }

    /// True when the value fits on one line after `key: ` or `- `.
    fn is_inline(&self) -> bool {
        match self {
            Self::Null
            | Self::Str(_)
            | Self::Bool(_)
            | Self::Int(_)
            | Self::Flow(_)
            | Self::Quoted(_)
            | Self::Annotated { .. } => true,
            Self::Seq(items) => items.is_empty(),
            Self::Map(entries) => entries.is_empty(),
        }
    }
}

/// Render a document with a trailing newline.
#[must_use]
pub fn render_yaml(value: &Yaml) -> String {
    let mut out = String::new();
    emit_node(value, 0, &mut out);
    out
}

/// Quote a scalar only when plain style would be unsafe or ambiguous.
#[must_use]
pub fn quote_scalar(value: &str) -> String {
    if can_emit_plain(value) {
        value.to_owned()
    } else {
        quote_double(value)
    }
}

/// Emit a nested node at an indent level.
fn emit_node(value: &Yaml, indent: usize, out: &mut String) {
    if value.is_inline() {
        emit_inline(value, out);
        out.push('\n');
        return;
    }
    match value {
        Yaml::Seq(items) => {
            for item in items {
                emit_seq_item(item, indent, out);
            }
        }
        Yaml::Map(entries) => {
            for (key, child) in entries {
                emit_map_entry(key, child, indent, out);
            }
        }
        Yaml::Null
        | Yaml::Str(_)
        | Yaml::Annotated { .. }
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::Quoted(_) => {}
    }
}

/// Emit one mapping entry.
fn emit_map_entry(key: &str, value: &Yaml, indent: usize, out: &mut String) {
    push_indent(indent, out);
    out.push_str(&quote_scalar(key));
    match value {
        Yaml::Null => out.push_str(":\n"),
        inline if inline.is_inline() => {
            out.push_str(": ");
            emit_inline(inline, out);
            out.push('\n');
        }
        nested => {
            out.push_str(":\n");
            emit_node(nested, indent + 1, out);
        }
    }
}

/// Emit one sequence item.
fn emit_seq_item(item: &Yaml, indent: usize, out: &mut String) {
    push_indent(indent, out);
    match item {
        Yaml::Map(entries) if !entries.is_empty() => {
            if let Some(((first_key, first_value), rest)) = entries.split_first() {
                out.push_str("- ");
                emit_first_entry(first_key, first_value, indent, out);
                for (key, child) in rest {
                    emit_map_entry(key, child, indent + 1, out);
                }
            }
        }
        Yaml::Seq(items) if !items.is_empty() => {
            out.push_str("-\n");
            emit_node(item, indent + 1, out);
        }
        Yaml::Null => out.push_str("-\n"),
        inline => {
            out.push_str("- ");
            emit_inline(inline, out);
            out.push('\n');
        }
    }
}

/// Emit the first mapping entry of a sequence item after `- `.
fn emit_first_entry(key: &str, value: &Yaml, indent: usize, out: &mut String) {
    out.push_str(&quote_scalar(key));
    match value {
        Yaml::Null => out.push_str(":\n"),
        inline if inline.is_inline() => {
            out.push_str(": ");
            emit_inline(inline, out);
            out.push('\n');
        }
        nested => {
            out.push_str(":\n");
            emit_node(nested, indent + 2, out);
        }
    }
}

/// Emit an inline value (scalars and empty collections).
fn emit_inline(value: &Yaml, out: &mut String) {
    match value {
        Yaml::Str(text) => out.push_str(&quote_scalar(text)),
        Yaml::Quoted(text) => out.push_str(&quote_double(text)),
        Yaml::Annotated { value, comment } => {
            out.push_str(&quote_scalar(value));
            out.push_str(" # ");
            out.push_str(comment);
        }
        Yaml::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Yaml::Int(num) => out.push_str(&num.to_string()),
        Yaml::Seq(items) if items.is_empty() => out.push_str("[]"),
        Yaml::Map(entries) if entries.is_empty() => out.push_str("{}"),
        Yaml::Flow(items) => emit_flow(items, out),
        Yaml::Null | Yaml::Seq(_) | Yaml::Map(_) => {}
    }
}

/// Emit `[a, b]` with the renderer's scalar quoting.
fn emit_flow(items: &[String], out: &mut String) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&quote_scalar(item));
    }
    out.push(']');
}

/// Push 2-space indentation.
fn push_indent(indent: usize, out: &mut String) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

/// Plain style is safe only for simple single-line, unambiguous scalars.
fn can_emit_plain(value: &str) -> bool {
    if value.is_empty() || looks_like_non_string(value) {
        return false;
    }
    let Some(first) = value.chars().next() else {
        return false;
    };
    if first.is_whitespace() || is_indicator(first) {
        return false;
    }
    if value.chars().last().is_some_and(char::is_whitespace) {
        return false;
    }
    if value.contains(": ") || value.contains(" #") || value.contains('"') || value.ends_with(':') {
        return false;
    }
    !value.chars().any(char::is_control)
}

/// YAML structural indicator characters.
fn is_indicator(ch: char) -> bool {
    matches!(
        ch,
        '-' | '?'
            | ':'
            | ','
            | '['
            | ']'
            | '{'
            | '}'
            | '#'
            | '&'
            | '*'
            | '!'
            | '|'
            | '>'
            | '\''
            | '"'
            | '%'
            | '@'
            | '`'
    )
}

/// True for scalars a YAML parser would not read back as strings.
fn looks_like_non_string(value: &str) -> bool {
    const WORDS: &[&str] = &[
        "true", "false", "yes", "no", "y", "n", "on", "off", "null", "nil", "~",
    ];
    if WORDS.contains(&value.to_lowercase().as_str()) {
        return true;
    }
    if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
        return true;
    }
    let lowered = value.to_lowercase();
    if ["0x", ".inf", ".nan"]
        .iter()
        .any(|p| lowered.starts_with(p))
    {
        return true;
    }
    looks_like_date(value)
}

/// True for a leading `YYYY-MM-DD` timestamp shape.
fn looks_like_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 10
        && bytes.iter().take(4).all(u8::is_ascii_digit)
        && bytes.get(4) == Some(&b'-')
        && bytes.get(5).is_some_and(u8::is_ascii_digit)
        && bytes.get(6).is_some_and(u8::is_ascii_digit)
        && bytes.get(7) == Some(&b'-')
        && bytes.get(8).is_some_and(u8::is_ascii_digit)
        && bytes.get(9).is_some_and(u8::is_ascii_digit)
}

/// Push a `\uXXXX` escape for a control code point.
fn push_unicode_escape(code: u32, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push_str("\\u");
    for shift in [12_u32, 8, 4, 0] {
        let digit = HEX[((code >> shift) & 0xF) as usize];
        out.push(char::from(digit));
    }
}

/// Double-quote with minimal escaping; printable Unicode stays raw.
fn quote_double(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => push_unicode_escape(ch as u32, &mut out),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
