//! Quoted string and multiline string tokenization for [`Parser`].

use crate::toml_parser::Parser;
use crate::toml_scan::{TomlDiagnostic, TomlValue};

impl Parser {
    /// Parse a quoted string value: single-line or multi-line (basic or literal).
    pub(crate) fn parse_string(&mut self) -> Result<TomlValue, TomlDiagnostic> {
        let Some(quote) = self.peek() else {
            return Err(self.fail("expected_value"));
        };
        if self.peek_at(1) == Some(quote) && self.peek_at(2) == Some(quote) {
            self.parse_multiline_string(quote).map(TomlValue::Str)
        } else {
            self.parse_quoted_text().map(TomlValue::Str)
        }
    }

    /// Parse single-line quoted text; triple-quoted strings are rejected explicitly.
    pub(crate) fn parse_quoted_text(&mut self) -> Result<String, TomlDiagnostic> {
        let Some(quote) = self.bump() else {
            return Err(self.fail("expected_value"));
        };
        if self.peek() == Some(quote) && self.peek_at(1) == Some(quote) {
            return Err(self.fail("multiline_key_unsupported"));
        }
        let start = self.line;
        let mut out = String::new();
        let basic = quote == '"';
        loop {
            match self.bump() {
                Some(char) if char == quote => return Ok(out),
                Some('\\') if basic => out.push_str(&self.parse_escape()?),
                Some('\n') | None => {
                    return Err(TomlDiagnostic {
                        line: start,
                        problem: "unterminated_string".to_owned(),
                    });
                }
                Some(char) if char.is_control() && char != '\t' => {
                    return Err(self.fail("bad_string"));
                }
                Some(char) => out.push(char),
            }
        }
    }

    /// Parse a triple-quoted multi-line string (basic `"""` or literal `'''`).
    fn parse_multiline_string(&mut self, quote: char) -> Result<String, TomlDiagnostic> {
        self.consume_triple_quote();
        self.trim_opening_newline();
        let start = self.line;
        let mut out = String::new();
        let basic = quote == '"';
        loop {
            match self.peek() {
                None => {
                    return Err(TomlDiagnostic {
                        line: start,
                        problem: "unterminated_string".to_owned(),
                    });
                }
                Some(char) if char == quote => {
                    if self.consume_closing_quotes(quote, &mut out)? {
                        return Ok(out);
                    }
                }
                Some('\\') if basic => {
                    self.bump();
                    if self.is_line_continuation() {
                        self.trim_line_continuation();
                    } else {
                        out.push_str(&self.parse_escape()?);
                    }
                }
                Some('\r') => {
                    if self.peek_at(1) == Some('\n') {
                        out.push('\r');
                        self.bump();
                    } else {
                        return Err(self.fail("bad_string"));
                    }
                }
                Some(char) if char.is_control() && char != '\t' && char != '\n' => {
                    return Err(self.fail("bad_string"));
                }
                Some(char) => {
                    out.push(char);
                    self.bump();
                }
            }
        }
    }

    /// Consume the 3 opening delimiter quotes.
    fn consume_triple_quote(&mut self) {
        self.bump();
        self.bump();
        self.bump();
    }

    /// Trim an immediate opening newline (`\n` or `\r\n`).
    fn trim_opening_newline(&mut self) {
        if self.peek() == Some('\r') && self.peek_at(1) == Some('\n') {
            self.bump();
            self.bump();
        } else if self.peek() == Some('\n') {
            self.bump();
        }
    }

    /// Handle quotes inside or at the end of a multi-line string.
    fn consume_closing_quotes(
        &mut self,
        quote: char,
        out: &mut String,
    ) -> Result<bool, TomlDiagnostic> {
        let count = self.consecutive_quotes(quote);
        match count {
            1 => {
                out.push(quote);
                self.bump();
                Ok(false)
            }
            2 => {
                out.push(quote);
                out.push(quote);
                self.bump();
                self.bump();
                Ok(false)
            }
            3 => {
                self.consume_triple_quote();
                Ok(true)
            }
            4 => {
                out.push(quote);
                self.bump();
                self.consume_triple_quote();
                Ok(true)
            }
            5 => {
                out.push(quote);
                out.push(quote);
                self.bump();
                self.bump();
                self.consume_triple_quote();
                Ok(true)
            }
            _ => Err(self.fail("bad_string")),
        }
    }

    /// Count consecutive occurrences of `quote` starting at the cursor.
    fn consecutive_quotes(&self, quote: char) -> usize {
        let mut count = 0;
        while self.peek_at(count) == Some(quote) {
            count += 1;
        }
        count
    }

    /// Whether the backslash is followed by optional spaces then a newline.
    fn is_line_continuation(&self) -> bool {
        let mut offset = 0;
        while matches!(self.peek_at(offset), Some(' ' | '\t')) {
            offset += 1;
        }
        matches!(
            (self.peek_at(offset), self.peek_at(offset + 1)),
            (Some('\n'), _) | (Some('\r'), Some('\n'))
        )
    }

    /// Skip horizontal spaces, newline, and all subsequent whitespace.
    fn trim_line_continuation(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
        while let Some(ch) = self.peek() {
            if ch == '"' && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"') {
                break;
            }
            if matches!(ch, ' ' | '\t' | '\r' | '\n') {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Parse one basic-string escape after the backslash.
    fn parse_escape(&mut self) -> Result<String, TomlDiagnostic> {
        match self.bump() {
            Some('b') => Ok("\u{0008}".to_owned()),
            Some('t') => Ok("\t".to_owned()),
            Some('n') => Ok("\n".to_owned()),
            Some('f') => Ok("\u{000C}".to_owned()),
            Some('r') => Ok("\r".to_owned()),
            Some('"') => Ok("\"".to_owned()),
            Some('\\') => Ok("\\".to_owned()),
            Some('u') => self.parse_unicode(4),
            Some('U') => self.parse_unicode(8),
            _ => Err(self.fail("bad_escape")),
        }
    }

    /// Parse a fixed-width `\u`/`\U` escape into one character.
    fn parse_unicode(&mut self, width: usize) -> Result<String, TomlDiagnostic> {
        let mut digits = String::with_capacity(width);
        for _ in 0..width {
            match self.peek() {
                Some(char) if char.is_ascii_hexdigit() => {
                    digits.push(char);
                    self.bump();
                }
                _ => return Err(self.fail("bad_escape")),
            }
        }
        let scalar = u32::from_str_radix(&digits, 16).unwrap_or(u32::MAX);
        char::from_u32(scalar)
            .map_or_else(|| Err(self.fail("bad_escape")), |char| Ok(char.to_string()))
    }
}
