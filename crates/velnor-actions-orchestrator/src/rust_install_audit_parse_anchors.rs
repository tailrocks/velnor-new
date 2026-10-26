// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::anchor_token;

#[path = "rust_install_audit_parse_anchor_lex.rs"]
mod lex;
use lex::{
    FlowState, QuoteState, entry_flow_start, entry_quote_continuation, flow_separator_positions,
    mapping_separator, poison_partial_flow_anchors, sequence_prefix, strip_yaml_comment,
    tag_property_end,
};

pub(super) fn duplicate_anchor_names(text: &str) -> BTreeSet<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    let mut continuation_indent = None;
    let mut continued_quote: Option<QuoteState> = None;
    let lines: Vec<&str> = text.lines().collect();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if let Some(mut quote) = continued_quote.take() {
            quote.consume(line);
            if quote.is_active() {
                continued_quote = Some(quote);
            }
            at += 1;
            continue;
        }
        let line_indent = indent(line);
        if continuation_indent.is_some_and(|base| line.trim().is_empty() || line_indent > base) {
            at += 1;
            continue;
        }
        continuation_indent = None;

        let mut content = strip_yaml_comment(line.trim_start());
        if line_indent == 0 {
            if let Some(rest) = document_marker_tail(content, "...")
                && rest.trim().is_empty()
            {
                at += 1;
                continue;
            }
            if let Some(rest) = document_marker_tail(content, "---") {
                content = rest.trim_start();
            }
        }
        if content.is_empty() {
            at += 1;
            continue;
        }
        let content = strip_explicit_key_indicator(strip_sequence_markers(content));
        let sequence_prefix = sequence_prefix(line.trim_start(), line_indent);
        let mapping_indent = line_indent + sequence_prefix.width;
        let direct_indent = sequence_prefix.last_marker_column.unwrap_or(line_indent);
        if let Some(flow) = entry_flow_start(content) {
            let mut state = FlowState::default();
            state.consume(flow, false);
            if state.is_open() {
                let mut logical_entry = content.to_owned();
                while state.is_open() && at + 1 < lines.len() {
                    at += 1;
                    let continuation = state.consume(lines[at], true);
                    logical_entry.push('\n');
                    logical_entry.push_str(&continuation);
                }
                if state.is_open() {
                    poison_partial_flow_anchors(&logical_entry, &mut counts);
                }
                continuation_indent =
                    scan_entry(&logical_entry, &mut counts, direct_indent, mapping_indent);
            } else {
                continuation_indent =
                    scan_entry(content, &mut counts, direct_indent, mapping_indent);
            }
        } else {
            continued_quote = entry_quote_continuation(content);
            continuation_indent = scan_entry(content, &mut counts, direct_indent, mapping_indent);
        }
        at += 1;
    }
    counts
        .into_iter()
        .filter_map(|(name, count)| (count > 1).then_some(name))
        .collect()
}

fn strip_sequence_markers(mut content: &str) -> &str {
    loop {
        if content == "-" {
            return "";
        }
        let Some(rest) = content.strip_prefix("- ") else {
            return content;
        };
        content = rest.trim_start();
    }
}

fn scan_entry(
    content: &str,
    counts: &mut BTreeMap<String, usize>,
    direct_indent: usize,
    mapping_indent: usize,
) -> Option<usize> {
    let content = strip_explicit_key_indicator(content);
    let (rest, block_value) = scan_node_properties(content, counts);
    if block_value {
        return Some(direct_indent);
    }
    if rest.starts_with('{') || rest.starts_with('[') {
        scan_node_content(rest, counts);
        return None;
    }
    let Some(value) = mapping_value(rest) else {
        return is_plain_scalar(rest).then_some(direct_indent);
    };
    let (value, block_value) = scan_node_properties(value, counts);
    if block_value {
        return Some(mapping_indent);
    }
    if scan_node_content(value, counts) || is_plain_scalar(value) {
        Some(mapping_indent)
    } else {
        None
    }
}

fn scan_node(value: &str, counts: &mut BTreeMap<String, usize>) -> bool {
    let value = strip_explicit_key_indicator(value);
    let (rest, block_value) = scan_node_properties(value, counts);
    block_value | scan_node_content(rest, counts)
}

fn scan_node_content(rest: &str, counts: &mut BTreeMap<String, usize>) -> bool {
    let rest = rest.trim();
    let mut block_value = false;
    if let Some(flow) = rest
        .strip_prefix('{')
        .and_then(|body| body.strip_suffix('}'))
    {
        for entry in flow_entries(flow) {
            if let Some((key, value)) = flow_mapping_entry(entry) {
                block_value |= scan_node(key, counts);
                block_value |= scan_node(value, counts);
            } else {
                block_value |= scan_node(entry, counts);
            }
        }
    } else if let Some(flow) = rest
        .strip_prefix('[')
        .and_then(|body| body.strip_suffix(']'))
    {
        for item in flow_entries(flow) {
            block_value |= scan_node(item, counts);
        }
    }
    block_value
}

fn scan_node_properties<'a>(
    value: &'a str,
    counts: &mut BTreeMap<String, usize>,
) -> (&'a str, bool) {
    let mut rest = value.trim_start();
    loop {
        if let Some((anchor, tail)) = anchor_token(rest, '&') {
            if anchor.is_empty() {
                break;
            }
            *counts.entry(anchor.to_owned()).or_default() += 1;
            rest = tail.trim_start();
        } else if let Some(after_tag) = rest.strip_prefix('!') {
            let end = tag_property_end(after_tag);
            if end == 0 {
                break;
            }
            rest = after_tag[end..].trim_start();
        } else {
            break;
        }
    }
    (rest, is_block_scalar(rest))
}

fn mapping_value(content: &str) -> Option<&str> {
    mapping_separator(content).map(|index| content[index + 1..].trim_start())
}

fn flow_entries(value: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0;
    for index in flow_separator_positions(value, ',') {
        entries.push(value[start..index].trim());
        start = index + 1;
    }
    let last = value[start..].trim();
    if !last.is_empty() {
        entries.push(last);
    }
    entries
}

fn flow_mapping_entry(entry: &str) -> Option<(&str, &str)> {
    let index = flow_separator_positions(entry, ':').into_iter().next()?;
    Some((entry[..index].trim(), entry[index + 1..].trim()))
}

fn strip_explicit_key_indicator(value: &str) -> &str {
    value.strip_prefix('?').map_or(value, |rest| {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            rest.trim_start()
        } else {
            value
        }
    })
}

fn is_block_scalar(value: &str) -> bool {
    let Some(indicator) = value.chars().next() else {
        return false;
    };
    if !matches!(indicator, '|' | '>') {
        return false;
    }
    value.chars().nth(1).is_none_or(|next| {
        next.is_whitespace() || next == '+' || next == '-' || next.is_ascii_digit()
    })
}

fn is_plain_scalar(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    if matches!(first, '[' | '{' | '\'' | '"' | '|' | '>' | '*' | '&' | '!') {
        return false;
    }
    if matches!(first, '-' | '?' | ':') {
        return value[first.len_utf8()..]
            .chars()
            .next()
            .is_some_and(|next| !next.is_whitespace());
    }
    true
}

fn document_marker_tail<'a>(content: &'a str, marker: &str) -> Option<&'a str> {
    let rest = content.strip_prefix(marker)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then_some(rest)
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}
