// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::iter::Peekable;
use std::str::CharIndices;

const MAX_DEPTH: usize = 64;
const PATH_SPECIAL: &[char] = &[
    '.', ' ', '\'', '/', '"', '[', ']', '(', ')', '\t', '\n', '\r', '\u{c}', '\u{8}', '\\',
    '\u{85}', '\u{2028}', '\u{2029}',
];

pub(super) struct Exceeded {
    pub message: String,
    pub probe: String,
}

enum Frame {
    Object { key: String, expects_key: bool },
    Array { index: Option<usize> },
}

impl Frame {
    fn closer(&self) -> char {
        match self {
            Self::Object { .. } => '}',
            Self::Array { .. } => ']',
        }
    }
}

struct Scanner<'a> {
    chars: Peekable<CharIndices<'a>>,
    line: usize,
    column: usize,
}

impl Scanner<'_> {
    fn advance(&mut self) -> Option<(usize, char)> {
        let (offset, c) = self.chars.next()?;
        match c {
            '\n' => self.newline(),
            '\r' if !matches!(self.chars.peek(), Some((_, '\n'))) => self.newline(),
            '\r' => {}
            _ => self.column += c.len_utf16(),
        }
        Some((offset, c))
    }

    fn newline(&mut self) {
        self.line += 1;
        self.column = 0;
    }

    fn string(&mut self) -> String {
        let mut units: Vec<u16> = Vec::new();
        while let Some((_, c)) = self.advance() {
            match c {
                '"' => break,
                '\\' => match self.advance().map(|(_, escaped)| escaped) {
                    Some('u') => units.push(self.hex_unit()),
                    Some(escaped) => units.push(unescape(escaped)),
                    None => break,
                },
                _ => {
                    let mut buffer = [0; 2];
                    units.extend_from_slice(c.encode_utf16(&mut buffer));
                }
            }
        }
        String::from_utf16_lossy(&units)
    }

    fn hex_unit(&mut self) -> u16 {
        let mut unit = 0u16;
        for _ in 0..4 {
            let digit = self
                .advance()
                .and_then(|(_, c)| c.to_digit(16))
                .unwrap_or(0);
            unit = (unit << 4) | u16::try_from(digit).unwrap_or(0);
        }
        unit
    }

    fn skip_scalar(&mut self) {
        while let Some((_, next)) = self.chars.peek() {
            if next.is_whitespace() || matches!(next, ',' | ']' | '}' | ':') {
                break;
            }
            self.advance();
        }
    }
}

fn unescape(escaped: char) -> u16 {
    match escaped {
        'b' => 8,
        'f' => 12,
        'n' => 10,
        'r' => 13,
        't' => 9,
        other => u16::try_from(u32::from(other)).unwrap_or(0xFFFD),
    }
}

fn bump(index: &mut Option<usize>) {
    *index = Some(index.map_or(0, |current| current + 1));
}

fn element_started(frames: &mut [Frame]) {
    if let Some(Frame::Array { index }) = frames.last_mut() {
        bump(index);
    }
}

fn path(frames: &[Frame]) -> String {
    let mut path = String::new();
    for frame in frames {
        match frame {
            Frame::Array { index } => {
                let _ = write!(path, "[{}]", index.unwrap_or_default());
            }
            Frame::Object { key, .. } if key.contains(PATH_SPECIAL) => {
                path.push_str("['");
                escape_name(key, &mut path);
                path.push_str("']");
            }
            Frame::Object { key, .. } => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(key);
            }
        }
    }
    path
}

fn escape_name(name: &str, out: &mut String) {
    for c in name.chars() {
        match c {
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '\u{8}' => out.push_str("\\b"),
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\u{85}' | '\u{2028}' | '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
}

pub(super) fn exceeded(json: &str) -> Option<Exceeded> {
    let mut scanner = Scanner {
        chars: json.char_indices().peekable(),
        line: 1,
        column: 0,
    };
    let mut frames: Vec<Frame> = Vec::new();
    while let Some((offset, c)) = scanner.advance() {
        match c {
            '"' => {
                let text = scanner.string();
                match frames.last_mut() {
                    Some(Frame::Object { key, expects_key }) if *expects_key => {
                        *key = text;
                        *expects_key = false;
                    }
                    Some(Frame::Array { index }) => bump(index),
                    Some(Frame::Object { .. }) => {}
                    None => return None,
                }
            }
            ',' => {
                if let Some(Frame::Object { expects_key, .. }) = frames.last_mut() {
                    *expects_key = true;
                }
            }
            '[' | '{' => {
                element_started(&mut frames);
                if frames.len() == MAX_DEPTH {
                    let closers: String = frames.iter().rev().map(Frame::closer).collect();
                    return Some(Exceeded {
                        message: format!(
                            "The reader's MaxDepth of {MAX_DEPTH} has been exceeded. Path '{}', line {}, position {}.",
                            path(&frames),
                            scanner.line,
                            scanner.column
                        ),
                        probe: format!("{}0{closers}", &json[..offset]),
                    });
                }
                frames.push(if c == '[' {
                    Frame::Array { index: None }
                } else {
                    Frame::Object {
                        key: String::new(),
                        expects_key: true,
                    }
                });
            }
            ']' | '}' => {
                frames.pop();
                if frames.is_empty() {
                    return None;
                }
            }
            ':' => {}
            c if c.is_whitespace() => {}
            _ => {
                if frames.is_empty() {
                    return None;
                }
                element_started(&mut frames);
                scanner.skip_scalar();
            }
        }
    }
    None
}
