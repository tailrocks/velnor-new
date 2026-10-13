//! YAML output primitives for the typed workflow tree.

use super::Yaml;

/// Render a document with a trailing newline.
#[must_use]
pub(super) fn render_yaml(value: &Yaml) -> String {
    let mut out = String::new();
    emit_node(value, 0, &mut out);
    out
}

/// Quote a scalar only when plain style would be unsafe or ambiguous.
#[must_use]
pub(super) fn quote_scalar(value: &str) -> String {
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
        Yaml::Map(entries) | Yaml::AnchoredMap { entries, .. } => {
            for (key, child) in entries {
                emit_map_entry(key, child, indent, out);
            }
        }
        Yaml::Null
        | Yaml::Str(_)
        | Yaml::AnchoredScalar { .. }
        | Yaml::Alias(_)
        | Yaml::Annotated { .. }
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::FlowMap(_)
        | Yaml::Quoted(_) => {}
    }
}

/// Emit one mapping entry.
fn emit_map_entry(key: &str, value: &Yaml, indent: usize, out: &mut String) {
    push_indent(indent, out);
    out.push_str(&quote_scalar(key));
    match value {
        Yaml::Null => out.push_str(":\n"),
        Yaml::Alias(name) => {
            out.push_str(": *");
            out.push_str(name.as_str());
            out.push('\n');
        }
        Yaml::AnchoredMap { name, entries } if !entries.is_empty() => {
            out.push_str(": &");
            out.push_str(name.as_str());
            out.push('\n');
            for (nested_key, nested_value) in entries {
                emit_map_entry(nested_key, nested_value, indent + 1, out);
            }
        }
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
        Yaml::AnchoredMap { name, entries } if !entries.is_empty() => {
            out.push_str("- &");
            out.push_str(name.as_str());
            out.push('\n');
            for (key, value) in entries {
                emit_map_entry(key, value, indent + 1, out);
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
        Yaml::Alias(name) => {
            out.push_str(": *");
            out.push_str(name.as_str());
            out.push('\n');
        }
        Yaml::AnchoredMap { name, entries } if !entries.is_empty() => {
            out.push_str(": &");
            out.push_str(name.as_str());
            out.push('\n');
            for (nested_key, nested_value) in entries {
                emit_map_entry(nested_key, nested_value, indent + 2, out);
            }
        }
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
        Yaml::AnchoredScalar { name, value } => {
            out.push('&');
            out.push_str(name.as_str());
            out.push(' ');
            out.push_str(&quote_scalar(value));
        }
        Yaml::AnchoredMap { name, entries } if entries.is_empty() => {
            out.push('&');
            out.push_str(name.as_str());
            out.push_str(" {}");
        }
        Yaml::Alias(name) => {
            out.push('*');
            out.push_str(name.as_str());
        }
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
        Yaml::FlowMap(entries) => emit_flow_map(entries, out),
        Yaml::Null | Yaml::Seq(_) | Yaml::Map(_) | Yaml::AnchoredMap { .. } => {}
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

/// Emit `{key: value}` with double-quoted scalars safe in flow context.
fn emit_flow_map(entries: &[(String, String)], out: &mut String) {
    out.push('{');
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&quote_double(key));
        out.push_str(": ");
        out.push_str(&quote_double(value));
    }
    out.push('}');
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

#[cfg(test)]
mod tests {
    use crate::yaml::{Yaml, render_yaml};

    #[test]
    fn flow_map_quotes_collection_delimiters_and_escapes() {
        let document = Yaml::FlowMap(vec![(
            "key,[]{}:? #".to_owned(),
            "value,[]{}: # \"quoted\" \\ line\nnext".to_owned(),
        )]);

        assert_eq!(
            render_yaml(&document),
            "{\"key,[]{}:? #\": \"value,[]{}: # \\\"quoted\\\" \\\\ line\\nnext\"}\n"
        );
    }
}
