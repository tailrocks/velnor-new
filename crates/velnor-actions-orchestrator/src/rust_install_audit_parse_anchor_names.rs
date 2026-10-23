//! Document-wide YAML anchor-name scan for the bounded emitted-workflow reader.

use std::collections::{BTreeMap, BTreeSet};

pub(super) struct AnchorScan {
    pub(super) duplicate_names: BTreeSet<String>,
    pub(super) malformed_tag: bool,
}

pub(super) fn scan(text: &str) -> AnchorScan {
    let mut declarations = BTreeMap::<String, usize>::new();
    let mut block_parent = None;
    let mut flow = FlowScanner::default();
    for line in text.lines() {
        let line_indent = indent(line);
        if let Some(parent) = block_parent {
            if line.trim().is_empty() || line_indent > parent {
                continue;
            }
            block_parent = None;
        }

        let continues_nested_syntax = flow.in_quote() || flow.in_flow();
        flow.scan_line(line, &mut declarations);
        if continues_nested_syntax || flow.in_quote() || flow.in_flow() {
            // The block-level scan cannot distinguish continued quoted or flow text.
            continue;
        }
        let content = strip_comment(line).trim_start();
        if content.is_empty() {
            continue;
        }
        let sequence_value = content
            .strip_prefix('-')
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
        let sequence_entry = sequence_value.is_some();
        let mut node = sequence_value.unwrap_or(content).trim_start();
        if sequence_value.is_some() {
            node = node_after_properties(node);
        }
        if let Some(explicit_key) = node.strip_prefix('?')
            && (explicit_key.is_empty() || explicit_key.starts_with(char::is_whitespace))
        {
            node = explicit_key.trim_start();
        }

        let mapping = mapping_value(node);
        let value = mapping.unwrap_or(node);
        let block_parent_indent = if sequence_entry && mapping.is_some() {
            line_indent + 2
        } else {
            line_indent
        };
        let value = node_after_properties(value.trim_start());
        if is_block_header(value) || is_plain_scalar(value) {
            block_parent = Some(block_parent_indent);
        }
    }

    AnchorScan {
        duplicate_names: declarations
            .into_iter()
            .filter_map(|(name, count)| (count > 1).then_some(name))
            .collect(),
        malformed_tag: flow.malformed_tag(),
    }
}

#[cfg(test)]
pub(super) fn duplicate_names(text: &str) -> BTreeSet<String> {
    scan(text).duplicate_names
}

pub(super) fn anchor_name_after_ampersand(value: &str) -> Option<&str> {
    let (name, rest) = anchor_value_after_ampersand(value)?;
    rest.is_empty().then_some(name)
}

pub(super) fn anchor_value_after_ampersand(value: &str) -> Option<(&str, &str)> {
    let value = strip_comment(value).trim_end();
    let (name, rest) = name_prefix(value);
    (!name.is_empty() && valid_boundary(rest)).then_some((name, rest.trim_start()))
}

pub(super) fn alias_name_after_asterisk(value: &str) -> Option<&str> {
    let value = strip_comment(value).trim_end().strip_prefix('*')?;
    let (name, rest) = name_prefix(value);
    (!name.is_empty() && rest.is_empty()).then_some(name)
}

fn anchor_prefix(value: &str) -> Option<(&str, &str)> {
    let (name, rest) = name_prefix(value.strip_prefix('&')?);
    (!name.is_empty() && valid_boundary(rest)).then_some((name, rest))
}

fn name_prefix(value: &str) -> (&str, &str) {
    let end = value
        .char_indices()
        .find_map(|(index, ch)| (!is_name_char(ch)).then_some(index))
        .unwrap_or(value.len());
    let name = &value[..end];
    let rest = &value[end..];
    if name.is_empty() || !valid_name(name) {
        ("", value)
    } else {
        (name, rest)
    }
}

fn valid_name(name: &str) -> bool {
    name.chars().all(is_name_char)
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn valid_boundary(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

fn record(declarations: &mut BTreeMap<String, usize>, name: &str) {
    *declarations.entry(name.to_owned()).or_default() += 1;
}

#[path = "rust_install_audit_parse_anchor_flow.rs"]
mod flow;
use flow::FlowScanner;

fn node_after_properties(mut value: &str) -> &str {
    loop {
        value = value.trim_start();
        if let Some((_, rest)) = anchor_prefix(value) {
            value = rest;
        } else if value.starts_with('!')
            && let Some(end) = flow::tag_end(value, 0)
        {
            value = &value[end..];
        } else {
            return value;
        }
    }
}

fn is_plain_scalar(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    if matches!(first, '[' | '{' | '\'' | '"' | '|' | '>' | '*' | '&' | '!') {
        return false;
    }
    !matches!(first, '-' | '?' | ':') || !flow::indicator_boundary(value, 0)
}

fn mapping_value(content: &str) -> Option<&str> {
    let key_start = content.len() - node_after_properties(content).len();
    let mut quote = None;
    let mut escaped = false;
    let mut characters = content.char_indices().peekable();
    while let Some((index, ch)) = characters.next() {
        if let Some(active) = quote {
            if active == '\''
                && ch == '\''
                && characters.peek().is_some_and(|(_, next)| *next == '\'')
            {
                characters.next();
                continue;
            }
            if active == '"' && ch == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if ch == active && !escaped {
                quote = None;
            }
            escaped = false;
            continue;
        }
        match ch {
            '\'' | '"' if index == key_start => quote = Some(ch),
            ':' if content[index + ch.len_utf8()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace) =>
            {
                return Some(&content[index + ch.len_utf8()..]);
            }
            _ => {}
        }
    }
    None
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut previous = None;
    let mut characters = line.char_indices().peekable();
    while let Some((index, ch)) = characters.next() {
        if let Some(active) = quote {
            if active == '"' {
                if ch == '\\' {
                    escaped = !escaped;
                    previous = Some(ch);
                    continue;
                }
                if ch == '"' && !escaped {
                    quote = None;
                }
                escaped = false;
                previous = Some(ch);
                continue;
            }
            if ch == '\'' {
                if characters.peek().is_some_and(|(_, next)| *next == '\'') {
                    let _ = characters.next();
                    previous = Some('\'');
                    continue;
                }
                quote = None;
            }
        } else {
            match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    escaped = false;
                }
                '#' if previous.is_none_or(char::is_whitespace) => return &line[..index],
                _ => {}
            }
        }
        previous = Some(ch);
    }
    line
}

fn is_block_header(value: &str) -> bool {
    let Some(indicator) = value.chars().next() else {
        return false;
    };
    if !matches!(indicator, '|' | '>') {
        return false;
    }
    value[1..]
        .chars()
        .all(|ch| matches!(ch, '+' | '-' | '1'..='9' | ' ' | '\t'))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}
