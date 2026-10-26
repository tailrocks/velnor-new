// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{mapping_value, scan_node_properties};

pub(super) fn entry_flow_start(content: &str) -> Option<&str> {
    let mut ignored = BTreeMap::new();
    let (rest, block_value) = scan_node_properties(content, &mut ignored);
    if block_value {
        return None;
    }
    let value = if rest.starts_with('{') || rest.starts_with('[') {
        rest
    } else {
        mapping_value(rest)?
    };
    let (value, block_value) = scan_node_properties(value, &mut ignored);
    (!block_value && (value.starts_with('{') || value.starts_with('['))).then_some(value)
}

pub(super) fn entry_quote_continuation(content: &str) -> Option<QuoteState> {
    let mut ignored = BTreeMap::new();
    let (rest, block_value) = scan_node_properties(content, &mut ignored);
    if block_value {
        return None;
    }
    let value = mapping_value(rest).unwrap_or(rest);
    let (value, block_value) = scan_node_properties(value, &mut ignored);
    if block_value {
        return None;
    }
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let mut state = QuoteState {
        quote: Some(quote),
        escaped: false,
        active: true,
    };
    state.consume(&value[quote.len_utf8()..]);
    state.is_active().then_some(state)
}

pub(super) struct SequencePrefix {
    pub width: usize,
    pub last_marker_column: Option<usize>,
}

pub(super) fn sequence_prefix(mut content: &str, mut column: usize) -> SequencePrefix {
    let mut width = 0;
    let mut last_marker_column = None;
    loop {
        let Some(after_marker) = content.strip_prefix('-') else {
            return SequencePrefix {
                width,
                last_marker_column,
            };
        };
        let separator = after_marker.len() - after_marker.trim_start().len();
        if separator == 0 {
            return SequencePrefix {
                width,
                last_marker_column,
            };
        }
        last_marker_column = Some(column);
        let marker_width = 1 + separator;
        width += marker_width;
        column += marker_width;
        content = after_marker.trim_start();
    }
}

pub(super) struct FlowState {
    collections: Vec<char>,
    quote: Option<char>,
    escaped: bool,
    node_start: bool,
    property: Option<NodeProperty>,
    completed_node: bool,
}

#[derive(Clone, Copy)]
enum NodeProperty {
    Anchor,
    Tag,
    VerbatimTag,
}

impl NodeProperty {
    fn ends_at(self, character: char) -> bool {
        match self {
            Self::Anchor => super::super::is_anchor_delimiter(character),
            Self::Tag => character.is_whitespace() || super::super::is_flow_indicator(character),
            Self::VerbatimTag => character == '>' || character.is_whitespace(),
        }
    }

    fn consumes_delimiter(self, character: char) -> bool {
        character.is_whitespace() || matches!(self, Self::VerbatimTag)
    }
}

impl Default for FlowState {
    fn default() -> Self {
        Self {
            collections: Vec::new(),
            quote: None,
            escaped: false,
            node_start: true,
            property: None,
            completed_node: false,
        }
    }
}

impl FlowState {
    pub(super) fn is_open(&self) -> bool {
        !self.collections.is_empty() || self.quote.is_some()
    }

    fn is_mapping_colon(&self, value: &str, index: usize) -> bool {
        self.completed_node || mapping_colon(value, index)
    }

    fn can_delimit(&self) -> bool {
        self.quote.is_none() && self.property.is_none()
    }

    fn end_property_at(&mut self, character: char) -> bool {
        let Some(property) = self.property.filter(|property| property.ends_at(character)) else {
            return false;
        };
        self.property = None;
        property.consumes_delimiter(character)
    }

    pub(super) fn consume(&mut self, value: &str, strip_comments: bool) -> String {
        let mut clean = String::with_capacity(value.len());
        for (index, character) in value.char_indices() {
            if strip_comments
                && self.quote.is_none()
                && character == '#'
                && (index == 0 || value[..index].ends_with(char::is_whitespace))
            {
                break;
            }
            self.consume_node_character(value, index, character);
            clean.push(character);
        }
        clean
    }

    fn consume_node_character(&mut self, value: &str, index: usize, character: char) {
        if let Some(active) = self.quote {
            if active == '"' && character == '\\' && !self.escaped {
                self.escaped = true;
                return;
            }
            if is_doubled_quote(value, index, active, character) {
                self.escaped = true;
                return;
            }
            if character == active && !self.escaped {
                self.quote = None;
                self.node_start = false;
                self.completed_node = true;
            }
            self.escaped = false;
            return;
        }
        if self.end_property_at(character) || self.property.is_some() {
            return;
        }
        if character.is_whitespace() {
            return;
        }
        if character != ':' {
            self.completed_node = false;
        }
        if self.node_start {
            if matches!(character, '?' | '-') && next_is_space_or_end(value, index, character) {
                return;
            }
            match character {
                '&' => {
                    self.property = Some(NodeProperty::Anchor);
                    return;
                }
                '!' => {
                    self.property = Some(if value[index + 1..].starts_with('<') {
                        NodeProperty::VerbatimTag
                    } else {
                        NodeProperty::Tag
                    });
                    return;
                }
                '\'' | '"' => {
                    self.quote = Some(character);
                    self.escaped = false;
                    return;
                }
                '[' | '{' => {
                    self.collections.push(character);
                    self.node_start = true;
                    return;
                }
                _ => self.node_start = false,
            }
        }
        match character {
            ']' if self.collections.last() == Some(&'[') => {
                self.collections.pop();
                self.node_start = false;
                self.completed_node = true;
            }
            '}' if self.collections.last() == Some(&'{') => {
                self.collections.pop();
                self.node_start = false;
                self.completed_node = true;
            }
            ',' if !self.collections.is_empty() => self.node_start = true,
            ':' if self.is_mapping_colon(value, index) => {
                self.node_start = true;
                self.completed_node = false;
            }
            _ => {}
        }
    }
}

pub(super) fn mapping_separator(value: &str) -> Option<usize> {
    let mut state = FlowState::default();
    for (index, character) in value.char_indices() {
        let depth = state.collections.len();
        let can_delimit = state.quote.is_none() && state.property.is_none();
        if can_delimit && depth == 0 && character == ':' && state.is_mapping_colon(value, index) {
            return Some(index);
        }
        state.consume_node_character(value, index, character);
    }
    None
}

pub(super) fn flow_separator_positions(value: &str, separator: char) -> Vec<usize> {
    let mut state = FlowState::default();
    let mut positions = Vec::new();
    for (index, character) in value.char_indices() {
        let depth = state.collections.len();
        let consumed_delimiter = state.end_property_at(character);
        let can_delimit = state.can_delimit();
        if can_delimit
            && depth == 0
            && character == separator
            && (separator != ':' || state.is_mapping_colon(value, index))
        {
            positions.push(index);
        }
        if can_delimit && depth == 0 && character == ',' {
            state.node_start = true;
            continue;
        }
        if !consumed_delimiter {
            state.consume_node_character(value, index, character);
        }
    }
    positions
}

pub(super) fn strip_yaml_comment(value: &str) -> &str {
    let mut state = FlowState::default();
    let mut block_value_started = false;
    for (index, character) in value.char_indices() {
        if state.quote.is_none()
            && character == '#'
            && (index == 0 || value[..index].ends_with(char::is_whitespace))
        {
            return value[..index].trim_end();
        }
        if state.quote.is_none()
            && state.property.is_none()
            && state.collections.is_empty()
            && character == ':'
            && state.is_mapping_colon(value, index)
        {
            if block_value_started {
                continue;
            }
            block_value_started = true;
        }
        state.consume_node_character(value, index, character);
    }
    value.trim_end()
}

fn mapping_colon(value: &str, index: usize) -> bool {
    value[index + 1..]
        .chars()
        .next()
        .is_none_or(|next| next.is_whitespace() || matches!(next, ',' | '[' | ']' | '{' | '}'))
}

fn next_is_space_or_end(value: &str, index: usize, character: char) -> bool {
    value[index + character.len_utf8()..]
        .chars()
        .next()
        .is_none_or(char::is_whitespace)
}

fn is_doubled_quote(value: &str, index: usize, quote: char, character: char) -> bool {
    quote == '\'' && character == '\'' && value.as_bytes().get(index + 1) == Some(&b'\'')
}

#[derive(Clone, Copy)]
pub(super) struct QuoteState {
    quote: Option<char>,
    escaped: bool,
    active: bool,
}

impl QuoteState {
    pub(super) fn is_active(self) -> bool {
        self.active
    }

    pub(super) fn consume(&mut self, value: &str) {
        for (index, character) in value.char_indices() {
            let Some(active) = self.quote else {
                break;
            };
            if active == '"' && character == '\\' && !self.escaped {
                self.escaped = true;
                continue;
            }
            if is_doubled_quote(value, index, active, character) {
                self.escaped = true;
                continue;
            }
            if character == active && !self.escaped {
                self.quote = None;
                self.active = false;
            }
            self.escaped = false;
        }
    }
}

pub(super) fn poison_partial_flow_anchors(value: &str, counts: &mut BTreeMap<String, usize>) {
    let mut state = FlowState::default();
    for (index, character) in value.char_indices() {
        if state.quote.is_none()
            && state.property.is_none()
            && state.node_start
            && character == '&'
            && let Some((anchor, _)) = super::super::anchor_token(&value[index..], '&')
            && !anchor.is_empty()
        {
            counts.insert(anchor.to_owned(), 2);
        }
        state.consume_node_character(value, index, character);
    }
}

pub(super) fn tag_property_end(value: &str) -> usize {
    if value.starts_with('<') {
        return value.find('>').map_or_else(
            || value.find(char::is_whitespace).unwrap_or(value.len()),
            |index| index + 1,
        );
    }
    value
        .char_indices()
        .find_map(|(index, character)| {
            (character.is_whitespace() || super::super::is_flow_indicator(character))
                .then_some(index)
        })
        .unwrap_or(value.len())
}
