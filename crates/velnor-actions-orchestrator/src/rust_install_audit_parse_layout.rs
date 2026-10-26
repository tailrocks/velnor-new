// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{StepRun, anchor_token};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MappingFieldState {
    Other,
    Map,
    AliasLeaf,
}

#[derive(Clone, Copy)]
pub(super) struct StepLayout {
    pub(super) mapping_indent: usize,
    pub(super) mapping_width: usize,
    pub(super) head_field: bool,
    pub(super) malformed_indent: bool,
}

pub(super) struct StepTopFieldsState<'a> {
    pub(super) run: &'a StepRun,
    pub(super) env_anchor: Option<String>,
    pub(super) env_alias: Option<String>,
    pub(super) env_field: MappingFieldState,
    pub(super) with_anchor: Option<String>,
    pub(super) with_alias: Option<String>,
    pub(super) with_field: MappingFieldState,
    pub(super) unexpected_nested: bool,
    pub(super) invalid_anchor: bool,
}

pub(super) fn normalize_top_field(
    key: &str,
    value: &str,
    state: &mut StepTopFieldsState<'_>,
) -> String {
    if key == "run" {
        return state.run.scalar.as_deref().unwrap_or(value).to_owned();
    }
    if key == "env" {
        state.with_field = MappingFieldState::Other;
        if let Some(anchor) = anchor_name(value, '&') {
            state.env_anchor = Some(anchor.to_owned());
            state.env_field = MappingFieldState::Map;
            return String::new();
        }
        if value.starts_with('&') {
            state.unexpected_nested = true;
            state.invalid_anchor = true;
            state.env_field = MappingFieldState::Other;
            return String::new();
        }
        if let Some(alias) = anchor_name(value, '*') {
            state.env_alias = Some(alias.to_owned());
            state.env_field = MappingFieldState::AliasLeaf;
            return String::new();
        }
        if value.starts_with('*') {
            state.unexpected_nested = true;
            state.invalid_anchor = true;
            state.env_field = MappingFieldState::Other;
            return String::new();
        }
        state.env_field = if value.is_empty() {
            MappingFieldState::Map
        } else {
            MappingFieldState::Other
        };
        return value.to_owned();
    }
    if key == "with" {
        state.env_field = MappingFieldState::Other;
        if let Some(anchor) = anchor_name(value, '&') {
            state.with_anchor = Some(anchor.to_owned());
            state.with_field = MappingFieldState::Map;
        } else if value.starts_with('&') {
            state.unexpected_nested = true;
            state.invalid_anchor = true;
            state.with_field = MappingFieldState::Other;
        } else if let Some(alias) = anchor_name(value, '*') {
            state.with_alias = Some(alias.to_owned());
            state.with_field = MappingFieldState::AliasLeaf;
        } else if value.starts_with('*') {
            state.unexpected_nested = true;
            state.invalid_anchor = true;
            state.with_field = MappingFieldState::Other;
        } else {
            state.with_field = if value.is_empty() {
                MappingFieldState::Map
            } else {
                MappingFieldState::Other
            };
        }
        return value.to_owned();
    }
    state.env_field = MappingFieldState::Other;
    state.with_field = MappingFieldState::Other;
    value.to_owned()
}

pub(super) fn update_mapping_alias_leaf(
    trimmed: &str,
    line_indent: usize,
    mapping_indent: usize,
    env_field: &mut MappingFieldState,
    unexpected_nested: &mut bool,
    invalid_anchor: &mut bool,
) -> bool {
    if *env_field != MappingFieldState::AliasLeaf {
        return false;
    }
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return true;
    }
    let next_top_field =
        line_indent == mapping_indent && (trimmed.contains(": ") || trimmed.ends_with(':'));
    if next_top_field {
        *env_field = MappingFieldState::Other;
    } else if line_indent > mapping_indent {
        *unexpected_nested = true;
        *invalid_anchor = true;
    }
    false
}

pub(super) fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

pub(super) fn sequence_head(line: &str) -> Option<&str> {
    let item = line.trim_start();
    if item == "-" {
        Some("")
    } else {
        item.strip_prefix("- ")
    }
}

pub(super) fn step_field_content<'a>(index: usize, line: &'a str, first_line: &'a str) -> &'a str {
    let content = if index == 0 {
        sequence_head(first_line).unwrap_or_default()
    } else {
        line.trim_start()
    };
    anchor_token(content.trim(), '&').map_or(content, |(_, tail)| tail.trim_start())
}

pub(super) fn step_layout(block: &[&str], sequence_width: usize) -> StepLayout {
    let item_indent = indent(block[0]);
    let head = sequence_head(block[0]).unwrap_or_default().trim();
    let head = anchor_token(head, '&').map_or(head, |(_, tail)| tail.trim_start());
    let head_field = head.contains(':');
    let (mapping_indent, mapping_width, malformed_indent) = if head_field {
        let mapping_indent = item_indent + 2;
        let block_run = head_run_is_block(head);
        let malformed_indent = block
            .iter()
            .skip(1)
            .find(|line| !line.trim().is_empty() && indent(line) > item_indent)
            .is_some_and(|line| {
                let child_indent = indent(line);
                child_indent != mapping_indent && !(block_run && child_indent > mapping_indent)
            });
        (mapping_indent, sequence_width, malformed_indent)
    } else {
        match block
            .iter()
            .skip(1)
            .find(|line| {
                !line.trim().is_empty() && indent(line) > item_indent && line.contains(':')
            })
            .map(|line| indent(line) - item_indent)
        {
            Some(width @ (1 | 2)) => (item_indent + width, width, false),
            Some(_) => (item_indent + 2, 2, true),
            None => (item_indent + 2, 2, false),
        }
    };
    StepLayout {
        mapping_indent,
        mapping_width,
        head_field,
        malformed_indent,
    }
}

fn head_run_is_block(head: &str) -> bool {
    let Some(value) = head.strip_prefix("run: ") else {
        return false;
    };
    let value = anchor_token(value, '&')
        .map_or(value, |(_, tail)| tail.trim_start())
        .trim_start();
    matches!(value.chars().next(), Some('|' | '>'))
        && value.chars().nth(1).is_none_or(|next| {
            next.is_whitespace() || next == '+' || next == '-' || next.is_ascii_digit()
        })
}

pub(super) fn anchor_name(value: &str, marker: char) -> Option<&str> {
    anchor_token(value, marker)
        .and_then(|(name, remainder)| (!name.is_empty() && remainder.is_empty()).then_some(name))
}

pub(super) fn step_name(block: &[&str]) -> String {
    block
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            let trimmed = step_field_content(index, line, block[0]);
            trimmed.strip_prefix("name: ")
        })
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_owned()
}
