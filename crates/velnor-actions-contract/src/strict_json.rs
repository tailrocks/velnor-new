// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::errors::ContractError;

pub const MAX_UNTRUSTED_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

pub const MAX_JSON_NESTING_DEPTH: usize = 128;

pub fn check_document_size(len: usize, limit: usize) -> Result<(), ContractError> {
    if len > limit {
        return Err(ContractError::DocumentTooLarge { size: len, limit });
    }
    Ok(())
}

pub fn parse_strict_json(text: &str) -> Result<serde_json::Value, ContractError> {
    parse_strict_json_with_limit(text, MAX_UNTRUSTED_DOCUMENT_BYTES)
}

pub fn parse_strict_json_with_limit(
    text: &str,
    limit: usize,
) -> Result<serde_json::Value, ContractError> {
    check_document_size(text.len(), limit)?;
    let mut scanner = Scanner {
        text,
        bytes: text.as_bytes(),
        pos: 0,
    };
    scanner.parse_document()?;
    serde_json::from_str(text).map_err(|err| ContractError::CanonicalJson(err.to_string()))
}

pub fn parse_strict_json_bytes(
    bytes: &[u8],
    limit: usize,
) -> Result<serde_json::Value, ContractError> {
    check_document_size(bytes.len(), limit)?;
    let text = std::str::from_utf8(bytes)
        .map_err(|err| ContractError::CanonicalJson(format!("malformed_json:{err}")))?;
    parse_strict_json_with_limit(text, limit)
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Scanner<'_> {
    fn parse_document(&mut self) -> Result<(), ContractError> {
        self.skip_ws();
        self.parse_value(0)?;
        self.skip_ws();
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(malformed("trailing_content"))
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<(), ContractError> {
        match self.peek() {
            Some(b'{') => {
                let nested = depth + 1;
                if nested >= MAX_JSON_NESTING_DEPTH {
                    return Err(too_deep(nested));
                }
                self.parse_object(nested)
            }
            Some(b'[') => {
                let nested = depth + 1;
                if nested >= MAX_JSON_NESTING_DEPTH {
                    return Err(too_deep(nested));
                }
                self.parse_array(nested)
            }
            Some(b'"') => self.parse_string().map(|_| ()),
            Some(b't') => self.parse_literal("true"),
            Some(b'f') => self.parse_literal("false"),
            Some(b'n') => self.parse_literal("null"),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err(malformed("unexpected_value")),
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<(), ContractError> {
        self.pos += 1;
        let mut keys = BTreeSet::new();
        self.skip_ws();
        if self.consume(b'}') {
            return Ok(());
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(malformed("expected_key"));
            }
            let key = self.parse_string()?;
            if !keys.insert(key.clone()) {
                return Err(ContractError::CanonicalJson(format!("duplicate_key:{key}")));
            }
            self.skip_ws();
            if !self.consume(b':') {
                return Err(malformed("expected_colon"));
            }
            self.skip_ws();
            self.parse_value(depth)?;
            self.skip_ws();
            if self.consume(b',') {
                continue;
            }
            if self.consume(b'}') {
                return Ok(());
            }
            return Err(malformed("expected_comma_or_close"));
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<(), ContractError> {
        self.pos += 1;
        self.skip_ws();
        if self.consume(b']') {
            return Ok(());
        }
        loop {
            self.skip_ws();
            self.parse_value(depth)?;
            self.skip_ws();
            if self.consume(b',') {
                continue;
            }
            if self.consume(b']') {
                return Ok(());
            }
            return Err(malformed("expected_comma_or_close"));
        }
    }

    fn parse_string(&mut self) -> Result<String, ContractError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(byte) = self.next() else {
                return Err(malformed("unterminated_string"));
            };
            match byte {
                b'"' => return Ok(out),
                b'\\' => out.push(self.parse_escape()?),
                0x00..=0x1F => return Err(malformed("control_in_string")),
                0x80..=0xFF => out.push(self.parse_utf8_char()?),
                _ => out.push(byte as char),
            }
        }
    }

    fn parse_utf8_char(&mut self) -> Result<char, ContractError> {
        self.pos -= 1;
        let rest = self
            .text
            .get(self.pos..)
            .ok_or_else(|| malformed("bad_utf8"))?;
        let ch = rest.chars().next().ok_or_else(|| malformed("bad_utf8"))?;
        self.pos += ch.len_utf8();
        Ok(ch)
    }

    fn parse_escape(&mut self) -> Result<char, ContractError> {
        match self.next() {
            Some(b'"') => Ok('"'),
            Some(b'\\') => Ok('\\'),
            Some(b'/') => Ok('/'),
            Some(b'b') => Ok('\u{0008}'),
            Some(b'f') => Ok('\u{000C}'),
            Some(b'n') => Ok('\n'),
            Some(b'r') => Ok('\r'),
            Some(b't') => Ok('\t'),
            Some(b'u') => self.parse_hex4(),
            _ => Err(malformed("bad_escape")),
        }
    }

    fn parse_hex4(&mut self) -> Result<char, ContractError> {
        let high = self.hex_value()?;
        if (0xD800..0xDC00).contains(&high) {
            if self.next() == Some(b'\\') && self.next() == Some(b'u') {
                let low = self.hex_value()?;
                if (0xDC00..0xE000).contains(&low) {
                    let scalar = 0x1_0000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                    return char::from_u32(scalar).ok_or_else(|| malformed("bad_escape"));
                }
            }
            return Err(malformed("bad_surrogate"));
        }
        char::from_u32(high).ok_or_else(|| malformed("bad_escape"))
    }

    fn hex_value(&mut self) -> Result<u32, ContractError> {
        if self.pos + 4 > self.bytes.len() {
            return Err(malformed("bad_escape"));
        }
        let mut value: u32 = 0;
        for _ in 0..4 {
            let digit = self.bytes[self.pos] as char;
            let nibble = digit.to_digit(16).ok_or_else(|| malformed("bad_escape"))?;
            value = value * 16 + nibble;
            self.pos += 1;
        }
        Ok(value)
    }

    fn parse_literal(&mut self, word: &str) -> Result<(), ContractError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(malformed("bad_literal"))
        }
    }

    fn parse_number(&mut self) -> Result<(), ContractError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if !self.take_digits() {
            return Err(malformed("bad_number"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !self.take_digits() {
                return Err(malformed("bad_number"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.take_digits() {
                return Err(malformed("bad_number"));
            }
        }
        if self.pos == start {
            return Err(malformed("bad_number"));
        }
        Ok(())
    }

    fn take_digits(&mut self) -> bool {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos > start
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn consume(&mut self, want: u8) -> bool {
        if self.peek() == Some(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }
}

fn malformed(problem: &str) -> ContractError {
    ContractError::CanonicalJson(format!("malformed_json:{problem}"))
}

fn too_deep(depth: usize) -> ContractError {
    ContractError::DocumentTooDeep {
        depth,
        limit: MAX_JSON_NESTING_DEPTH,
    }
}
