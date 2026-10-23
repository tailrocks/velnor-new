//! Bounded YAML flow and block-node lexical state for anchor discovery.

use std::collections::BTreeMap;

use super::{name_prefix, record};

#[derive(Default)]
pub(super) struct FlowScanner {
    stack: Vec<FlowFrame>,
    quote: Option<char>,
    escaped: bool,
    malformed_tag: bool,
}

struct FlowFrame {
    closing: char,
    node_start: bool,
}

impl FlowScanner {
    pub(super) fn in_flow(&self) -> bool {
        !self.stack.is_empty()
    }

    pub(super) fn in_quote(&self) -> bool {
        self.quote.is_some()
    }

    pub(super) fn malformed_tag(&self) -> bool {
        self.malformed_tag
    }

    pub(super) fn scan_line(&mut self, line: &str, declarations: &mut BTreeMap<String, usize>) {
        let mut characters = line.char_indices().peekable();
        let mut previous = None;
        let first_nonspace = line.find(|character: char| !character.is_whitespace());
        let mut root_node_start = self.stack.is_empty() && self.quote.is_none();
        while let Some((index, character)) = characters.next() {
            if self.consume_quoted(character, &mut characters) {
                previous = Some(character);
                continue;
            }

            if character == '#' && previous.is_none_or(char::is_whitespace) {
                break;
            }

            if self.stack.is_empty() {
                self.scan_root_character(
                    line,
                    (index, character),
                    first_nonspace == Some(index),
                    &mut characters,
                    &mut root_node_start,
                    declarations,
                );
            } else {
                self.scan_flow_character(line, index, character, &mut characters, declarations);
            }
            previous = Some(character);
        }
    }

    fn consume_quoted(
        &mut self,
        character: char,
        characters: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    ) -> bool {
        let Some(active) = self.quote else {
            return false;
        };
        if active == '"' {
            if character == '\\' {
                self.escaped = !self.escaped;
            } else {
                if character == '"' && !self.escaped {
                    self.quote = None;
                }
                self.escaped = false;
            }
        } else if character == '\'' {
            if characters.peek().is_some_and(|(_, next)| *next == '\'') {
                characters.next();
            } else {
                self.quote = None;
            }
        }
        true
    }

    fn scan_root_character(
        &mut self,
        line: &str,
        position: (usize, char),
        first_nonspace: bool,
        characters: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
        node_start: &mut bool,
        declarations: &mut BTreeMap<String, usize>,
    ) {
        let (index, character) = position;
        if character.is_whitespace() {
            return;
        }
        let sequence_indicator = first_nonspace && character == '-';
        let explicit_mapping_indicator = *node_start && matches!(character, '?' | ':');
        if (sequence_indicator || explicit_mapping_indicator) && indicator_boundary(line, index) {
            *node_start = true;
            return;
        }
        if *node_start {
            if matches!(character, '\'' | '"') {
                self.quote = Some(character);
                self.escaped = false;
                *node_start = false;
                return;
            }
            if character == '&'
                && let Some((name, end)) = flow_anchor_token(line, index)
            {
                record(declarations, name);
                consume_until(characters, end);
                return;
            }
            if character == '!' {
                if let Some(end) = tag_end(line, index) {
                    consume_until(characters, end);
                } else {
                    self.malformed_tag = true;
                    *node_start = false;
                }
                return;
            }
            if matches!(character, '{' | '[') {
                *node_start = false;
                self.stack.push(new_flow_frame(character));
                return;
            }
        }
        *node_start = character == ':' && block_mapping_boundary(line, index);
    }

    fn scan_flow_character(
        &mut self,
        line: &str,
        index: usize,
        character: char,
        characters: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
        declarations: &mut BTreeMap<String, usize>,
    ) {
        if matches!(character, '}' | ']') {
            self.close_flow(character);
            return;
        }
        let node_start = self.stack.last().is_some_and(|frame| frame.node_start);
        if node_start && matches!(character, '\'' | '"') {
            self.quote = Some(character);
            self.escaped = false;
            self.set_flow_node_start(false);
        } else if node_start && matches!(character, '{' | '[') {
            self.set_flow_node_start(false);
            self.stack.push(new_flow_frame(character));
        } else if node_start && character == '&' {
            self.scan_flow_anchor(line, index, characters, declarations);
        } else if node_start && character == '!' {
            self.scan_flow_tag(line, index, characters);
        } else {
            self.advance_flow_node(line, index, character);
        }
    }

    fn scan_flow_anchor(
        &mut self,
        line: &str,
        index: usize,
        characters: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
        declarations: &mut BTreeMap<String, usize>,
    ) {
        if let Some((name, end)) = flow_anchor_token(line, index) {
            record(declarations, name);
            consume_until(characters, end);
        } else {
            self.set_flow_node_start(false);
        }
    }

    fn scan_flow_tag(
        &mut self,
        line: &str,
        index: usize,
        characters: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    ) {
        if let Some(end) = tag_end(line, index) {
            consume_until(characters, end);
        } else {
            self.malformed_tag = true;
            self.set_flow_node_start(false);
        }
    }

    fn advance_flow_node(&mut self, line: &str, index: usize, character: char) {
        let Some(frame) = self.stack.last_mut() else {
            return;
        };
        match character {
            ',' => frame.node_start = true,
            ':' if frame.node_start || mapping_boundary(line, index) => frame.node_start = true,
            ch if ch.is_whitespace() => {}
            _ => frame.node_start = false,
        }
    }

    fn set_flow_node_start(&mut self, node_start: bool) {
        if let Some(frame) = self.stack.last_mut() {
            frame.node_start = node_start;
        }
    }

    fn close_flow(&mut self, character: char) {
        if self
            .stack
            .last()
            .is_some_and(|frame| frame.closing == character)
        {
            self.stack.pop();
        }
        self.set_flow_node_start(false);
    }
}

fn new_flow_frame(character: char) -> FlowFrame {
    FlowFrame {
        closing: if character == '{' { '}' } else { ']' },
        node_start: true,
    }
}

fn consume_until(characters: &mut std::iter::Peekable<std::str::CharIndices<'_>>, end: usize) {
    while characters
        .peek()
        .is_some_and(|(next_index, _)| *next_index < end)
    {
        characters.next();
    }
}

pub(super) fn indicator_boundary(line: &str, index: usize) -> bool {
    line[index + 1..]
        .chars()
        .next()
        .is_none_or(char::is_whitespace)
}

fn mapping_boundary(line: &str, index: usize) -> bool {
    line[index + 1..].chars().next().is_none_or(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | ']' | '}' | '&' | '!' | '\'' | '"' | '[' | '{' | '*' | '?'
            )
    })
}

pub(super) fn block_mapping_boundary(line: &str, index: usize) -> bool {
    line[index + 1..]
        .chars()
        .next()
        .is_none_or(char::is_whitespace)
}

fn flow_anchor_token(line: &str, ampersand: usize) -> Option<(&str, usize)> {
    let (name, rest) = name_prefix(&line[ampersand + 1..]);
    if name.is_empty()
        || !rest
            .chars()
            .next()
            .is_none_or(|ch| ch.is_whitespace() || matches!(ch, ',' | '[' | ']' | '{' | '}' | ':'))
    {
        return None;
    }
    Some((name, ampersand + 1 + name.len()))
}

pub(super) fn tag_end(line: &str, tag: usize) -> Option<usize> {
    if line[tag + 1..].starts_with('<') {
        let uri_start = tag + 2;
        return line[uri_start..]
            .find('>')
            .map(|close| uri_start + close + 1);
    }

    let mut end = tag + 1;
    for (offset, character) in line[tag + 1..].char_indices() {
        if character.is_whitespace() || matches!(character, ',' | '[' | ']' | '{' | '}') {
            break;
        }
        end = tag + 1 + offset + character.len_utf8();
    }
    (end > tag + 1).then_some(end)
}
