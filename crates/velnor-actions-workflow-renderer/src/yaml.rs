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
    render_yaml_with_indent(value, 2)
}

/// Render a compact document with one-space nesting indentation.
///
/// YAML permits any positive indentation width. This keeps the same node
/// order and scalar encoding as [`render_yaml`], while reducing repeated
/// structural whitespace for workflows near GitHub's file limit.
#[must_use]
pub fn render_yaml_compact(value: &Yaml) -> String {
    render_yaml_with_indent(value, 1)
}

fn render_yaml_with_indent(value: &Yaml, indent_width: usize) -> String {
    let mut out = String::new();
    emit_node(value, 0, indent_width, &mut out);
    out
}

/// Quote bare env paths in every `run:` scalar.
pub(crate) fn quote_run_values_in_yaml(node: Yaml) -> Yaml {
    match node {
        Yaml::Map(entries) => Yaml::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    if key == "run" {
                        if let Yaml::Str(line) = value {
                            (
                                key,
                                Yaml::Str(crate::commands::quote_run_line_env_paths(&line)),
                            )
                        } else {
                            (key, value)
                        }
                    } else {
                        (key, quote_run_values_in_yaml(value))
                    }
                })
                .collect(),
        ),
        Yaml::Seq(items) => Yaml::Seq(items.into_iter().map(quote_run_values_in_yaml).collect()),
        other => other,
    }
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
fn emit_node(value: &Yaml, indent: usize, indent_width: usize, out: &mut String) {
    if value.is_inline() {
        emit_inline(value, out);
        out.push('\n');
        return;
    }
    match value {
        Yaml::Seq(items) => {
            for item in items {
                emit_seq_item(item, indent, indent_width, out);
            }
        }
        Yaml::Map(entries) => {
            for (key, child) in entries {
                emit_map_entry(key, child, indent, indent_width, out);
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
fn emit_map_entry(key: &str, value: &Yaml, indent: usize, indent_width: usize, out: &mut String) {
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
            emit_node(nested, indent + indent_width, indent_width, out);
        }
    }
}

/// Emit one sequence item.
fn emit_seq_item(item: &Yaml, indent: usize, indent_width: usize, out: &mut String) {
    push_indent(indent, out);
    match item {
        Yaml::Map(entries) if !entries.is_empty() => {
            if let Some(((first_key, first_value), rest)) = entries.split_first() {
                out.push_str("- ");
                emit_first_entry(first_key, first_value, indent, indent_width, out);
                for (key, child) in rest {
                    emit_map_entry(key, child, indent + 2, indent_width, out);
                }
            }
        }
        Yaml::Seq(items) if !items.is_empty() => {
            out.push_str("-\n");
            emit_node(item, indent + 2, indent_width, out);
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
fn emit_first_entry(key: &str, value: &Yaml, indent: usize, indent_width: usize, out: &mut String) {
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
            emit_node(nested, indent + 2 + indent_width, indent_width, out);
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

/// Push the requested number of indentation spaces.
fn push_indent(indent: usize, out: &mut String) {
    for _ in 0..indent {
        out.push(' ');
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
    use std::error::Error;

    use super::{Yaml, render_yaml, render_yaml_compact};

    #[test]
    fn compact_render_preserves_multiline_run_scalar_and_structure() {
        let document = Yaml::Map(vec![(
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "build".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str("compile")),
                        ("run".to_owned(), Yaml::str("echo first\necho second")),
                    ])]),
                )]),
            )]),
        )]);

        let regular = render_yaml(&document);
        let compact = render_yaml_compact(&document);
        assert!(compact.len() < regular.len());
        assert!(regular.contains("run: \"echo first\\necho second\"\n"));
        assert!(compact.contains("run: \"echo first\\necho second\"\n"));
        assert!(compact.contains("jobs:\n build:\n  steps:\n   - name: compile\n     run:"));
    }

    #[test]
    fn compact_render_keeps_scalar_quoting_and_map_order() {
        let document = Yaml::Map(vec![
            ("on".to_owned(), Yaml::str("push")),
            ("timeout-minutes".to_owned(), Yaml::Int(5)),
            (
                "runs-on".to_owned(),
                Yaml::Flow(vec!["ubuntu-latest".to_owned()]),
            ),
        ]);

        assert_eq!(
            render_yaml_compact(&document),
            "\"on\": push\ntimeout-minutes: 5\nruns-on: [ubuntu-latest]\n"
        );
    }

    #[test]
    fn compact_workflow_matches_canonical_yaml_event_stream() -> Result<(), Box<dyn Error>> {
        let document = Yaml::Map(vec![(
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "build".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str("compile")),
                        ("run".to_owned(), Yaml::str("echo first\necho second")),
                        (
                            "env".to_owned(),
                            Yaml::Map(vec![("MODE".to_owned(), Yaml::str("safe"))]),
                        ),
                        (
                            "with".to_owned(),
                            Yaml::Map(vec![(
                                "args".to_owned(),
                                Yaml::Seq(vec![Yaml::str("one"), Yaml::str("two")]),
                            )]),
                        ),
                    ])]),
                )]),
            )]),
        )]);
        let canonical = render_yaml(&document);
        let compact = render_yaml_compact(&document);
        let canonical_events = yaml_events(&canonical)?;
        let compact_events = yaml_events(&compact)?;

        assert_eq!(compact_events, canonical_events);
        assert!(
            compact_events
                .iter()
                .any(|event| { event.contains("echo first\\necho second") })
        );
        Ok(())
    }

    fn yaml_events(source: &str) -> Result<Vec<String>, granit_parser::ScanError> {
        granit_parser::Parser::new_from_str(source)
            .map(|result| result.map(|(event, _span)| format!("{event:?}")))
            .collect()
    }
}
