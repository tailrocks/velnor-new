// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The strict JSON parse the legacy op applies to the request file: no duplicate keys, no
//! trailing content, no control characters in strings and a nesting limit.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::error::CiError;
use crate::write_request::internal;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 128;

pub(crate) fn parse(text: &str) -> Result<Value, CiError> {
    if text.len() > MAX_BYTES {
        return Err(internal(&format!(
            "document_too_large: {} bytes exceeds limit {MAX_BYTES}",
            text.len()
        )));
    }
    let mut scanner = Scanner {
        text,
        bytes: text.as_bytes(),
        pos: 0,
    };
    scanner.skip_ws();
    scanner.value(0)?;
    scanner.skip_ws();
    if scanner.pos != scanner.bytes.len() {
        return Err(malformed("trailing_content"));
    }
    serde_json::from_str(text).map_err(|error| json_error(&error.to_string()))
}

fn json_error(problem: &str) -> CiError {
    internal(&format!("canonical json error: {problem}"))
}

fn malformed(problem: &str) -> CiError {
    json_error(&format!("malformed_json:{problem}"))
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Scanner<'_> {
    fn value(&mut self, depth: usize) -> Result<(), CiError> {
        match self.peek() {
            Some(b'{') => self.nested(depth, Self::object),
            Some(b'[') => self.nested(depth, Self::array),
            Some(b'"') => self.string().map(drop),
            Some(b't') => self.literal("true"),
            Some(b'f') => self.literal("false"),
            Some(b'n') => self.literal("null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(malformed("unexpected_value")),
        }
    }

    fn nested(
        &mut self,
        depth: usize,
        body: fn(&mut Self, usize) -> Result<(), CiError>,
    ) -> Result<(), CiError> {
        let nested = depth + 1;
        if nested >= MAX_DEPTH {
            return Err(internal(&format!(
                "json_too_deep: depth {nested} exceeds limit {MAX_DEPTH}"
            )));
        }
        body(self, nested)
    }

    fn object(&mut self, depth: usize) -> Result<(), CiError> {
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
            let key = self.string()?;
            if !keys.insert(key.clone()) {
                return Err(json_error(&format!("duplicate_key:{key}")));
            }
            self.skip_ws();
            if !self.consume(b':') {
                return Err(malformed("expected_colon"));
            }
            self.skip_ws();
            self.value(depth)?;
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

    fn array(&mut self, depth: usize) -> Result<(), CiError> {
        self.pos += 1;
        self.skip_ws();
        if self.consume(b']') {
            return Ok(());
        }
        loop {
            self.skip_ws();
            self.value(depth)?;
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

    fn string(&mut self) -> Result<String, CiError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(byte) = self.next() else {
                return Err(malformed("unterminated_string"));
            };
            match byte {
                b'"' => return Ok(out),
                b'\\' => out.push(self.escape()?),
                0x00..=0x1F => return Err(malformed("control_in_string")),
                0x80..=0xFF => out.push(self.utf8_char()?),
                _ => out.push(char::from(byte)),
            }
        }
    }

    fn utf8_char(&mut self) -> Result<char, CiError> {
        self.pos -= 1;
        let rest = self
            .text
            .get(self.pos..)
            .ok_or_else(|| malformed("bad_utf8"))?;
        let ch = rest.chars().next().ok_or_else(|| malformed("bad_utf8"))?;
        self.pos += ch.len_utf8();
        Ok(ch)
    }

    fn escape(&mut self) -> Result<char, CiError> {
        match self.next() {
            Some(b'"') => Ok('"'),
            Some(b'\\') => Ok('\\'),
            Some(b'/') => Ok('/'),
            Some(b'b') => Ok('\u{0008}'),
            Some(b'f') => Ok('\u{000C}'),
            Some(b'n') => Ok('\n'),
            Some(b'r') => Ok('\r'),
            Some(b't') => Ok('\t'),
            Some(b'u') => self.hex4(),
            _ => Err(malformed("bad_escape")),
        }
    }

    fn hex4(&mut self) -> Result<char, CiError> {
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

    fn hex_value(&mut self) -> Result<u32, CiError> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| malformed("bad_escape"))?;
        let mut value: u32 = 0;
        for digit in digits {
            let nibble = char::from(*digit)
                .to_digit(16)
                .ok_or_else(|| malformed("bad_escape"))?;
            value = value * 16 + nibble;
        }
        self.pos += 4;
        Ok(value)
    }

    fn literal(&mut self, word: &str) -> Result<(), CiError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(malformed("bad_literal"))
        }
    }

    fn number(&mut self) -> Result<(), CiError> {
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if !self.digits() {
            return Err(malformed("bad_number"));
        }
        if self.consume(b'.') && !self.digits() {
            return Err(malformed("bad_number"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.digits() {
                return Err(malformed("bad_number"));
            }
        }
        Ok(())
    }

    fn digits(&mut self) -> bool {
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
        let found = self.peek() == Some(want);
        if found {
            self.pos += 1;
        }
        found
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }
}
