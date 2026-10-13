//! Character-cursor TOML parser behind [`crate::toml_scan::parse_toml`].
//!
//! Owns tokenization (sections, dotted keys, strings, arrays, inline
//! tables) with one-based line tracking; document assembly and
//! flattening stay in [`crate::toml_scan`].

use std::collections::BTreeSet;

use crate::toml_scan::{TomlDiagnostic, TomlDoc, TomlValue, flatten_value};

/// Character cursor with one-based line tracking.
pub(crate) struct Parser {
    /// Input characters.
    chars: Vec<char>,
    /// Cursor position.
    pos: usize,
    /// One-based current line.
    line: u32,
}

impl Parser {
    /// Cursor over `content` starting at line one.
    pub(crate) fn new(content: &str) -> Self {
        Self {
            chars: content.chars().collect(),
            pos: 0,
            line: 1,
        }
    }

    /// Parse one `[section]` or `[[table]]` header; return its path.
    pub(crate) fn parse_header(
        &mut self,
        doc: &mut TomlDoc,
        defined: &BTreeSet<Vec<String>>,
    ) -> Result<Vec<String>, TomlDiagnostic> {
        let line = self.line;
        self.bump();
        let array = self.eat('[');
        let path = self.parse_dotted()?;
        self.eat_space();
        if self.bump() != Some(']') || (array && self.bump() != Some(']')) {
            return Err(TomlDiagnostic {
                line,
                problem: "unterminated_section".to_owned(),
            });
        }
        if defined.contains(&path) {
            return Err(TomlDiagnostic {
                line,
                problem: "duplicate_key".to_owned(),
            });
        }
        self.end_of_line()?;
        doc.sections.push((path.clone(), line));
        Ok(path)
    }

    /// Parse one `dotted = value` entry under `section`.
    pub(crate) fn parse_entry(
        &mut self,
        section: &[String],
        doc: &mut TomlDoc,
        defined: &mut BTreeSet<Vec<String>>,
        tables: &BTreeSet<Vec<String>>,
    ) -> Result<(), TomlDiagnostic> {
        let line = self.line;
        let key = self.parse_dotted()?;
        self.eat_space();
        if self.bump() != Some('=') {
            return Err(self.fail("expected_key_equals_value"));
        }
        let value = self.parse_value()?;
        self.end_of_line()?;
        let mut path: Vec<String> = section.to_vec();
        path.extend(key);
        flatten_value(path, value, line, doc, defined, tables)
    }

    /// Parse one value: string, boolean, number, array, or inline table.
    fn parse_value(&mut self) -> Result<InlineValue, TomlDiagnostic> {
        self.eat_space();
        match self.peek() {
            Some('"' | '\'') => self.parse_string().map(InlineValue::Scalar),
            Some('[') => self.parse_array().map(InlineValue::Scalar),
            Some('{') => self.parse_inline_table(),
            Some(_) => self.parse_bare().map(InlineValue::Scalar),
            None => Err(self.fail("expected_value")),
        }
    }

    /// Parse a dotted (possibly quoted) key path.
    fn parse_dotted(&mut self) -> Result<Vec<String>, TomlDiagnostic> {
        let mut parts = vec![self.parse_key()?];
        loop {
            self.eat_space();
            if !self.eat('.') {
                return Ok(parts);
            }
            self.eat_space();
            parts.push(self.parse_key()?);
        }
    }

    /// Parse one bare or quoted key segment.
    fn parse_key(&mut self) -> Result<String, TomlDiagnostic> {
        match self.peek() {
            Some('"' | '\'') => self.parse_quoted_text(),
            Some(char) if is_bare_char(char) => {
                let mut key = String::new();
                while let Some(next) = self.peek() {
                    if !is_bare_char(next) {
                        break;
                    }
                    key.push(next);
                    self.bump();
                }
                Ok(key)
            }
            _ => Err(self.fail("expected_key")),
        }
    }

    /// Parse a quoted string value (basic or literal).
    fn parse_string(&mut self) -> Result<TomlValue, TomlDiagnostic> {
        self.parse_quoted_text().map(TomlValue::Str)
    }

    /// Parse quoted text; triple-quoted strings are rejected explicitly.
    fn parse_quoted_text(&mut self) -> Result<String, TomlDiagnostic> {
        let quote = self.bump().unwrap_or('\0');
        if self.peek() == Some(quote) && self.peek_at(1) == Some(quote) {
            return Err(self.fail("multiline_string_unsupported"));
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

    /// Parse `true`/`false` or an opaque bare scalar token.
    fn parse_bare(&mut self) -> Result<TomlValue, TomlDiagnostic> {
        let mut token = String::new();
        while let Some(char) = self.peek() {
            if !is_bare_value_char(char) {
                break;
            }
            token.push(char);
            self.bump();
        }
        match token.as_str() {
            "" => Err(self.fail("expected_value")),
            "true" => Ok(TomlValue::Bool(true)),
            "false" => Ok(TomlValue::Bool(false)),
            _ => Ok(TomlValue::Num(token)),
        }
    }

    /// Parse an array value across lines; commas separate items.
    fn parse_array(&mut self) -> Result<TomlValue, TomlDiagnostic> {
        self.bump();
        let mut items = Vec::new();
        loop {
            self.skip_trivia();
            if self.eat(']') {
                return Ok(TomlValue::Array(items));
            }
            if self.at_end() {
                return Err(self.fail("unterminated_array"));
            }
            items.push(self.parse_array_item()?);
            self.skip_trivia();
            if self.eat(',') {
                continue;
            }
            if self.eat(']') {
                return Ok(TomlValue::Array(items));
            }
            return Err(self.fail("expected_comma_or_close"));
        }
    }

    /// Parse one array item (scalars and arrays nest; tables do not).
    fn parse_array_item(&mut self) -> Result<TomlValue, TomlDiagnostic> {
        self.eat_space();
        match self.peek() {
            Some('"' | '\'') => self.parse_string(),
            Some('[') => self.parse_array(),
            Some('{') => Err(self.fail("table_in_array_unsupported")),
            Some(_) => self.parse_bare(),
            None => Err(self.fail("expected_value")),
        }
    }

    /// Parse an inline table into nested entries for flattening.
    fn parse_inline_table(&mut self) -> Result<InlineValue, TomlDiagnostic> {
        self.bump();
        let mut entries = Vec::new();
        loop {
            self.skip_trivia();
            if self.eat('}') {
                return Ok(InlineValue::Table(entries));
            }
            if self.at_end() {
                return Err(self.fail("unterminated_table"));
            }
            entries.push(self.parse_table_entry()?);
            self.skip_trivia();
            if self.eat(',') {
                continue;
            }
            if self.eat('}') {
                return Ok(InlineValue::Table(entries));
            }
            return Err(self.fail("expected_comma_or_close"));
        }
    }

    /// Parse one inline-table `dotted = value` entry with its key line.
    fn parse_table_entry(&mut self) -> Result<(Vec<String>, InlineValue, u32), TomlDiagnostic> {
        let line = self.line;
        let key = self.parse_dotted()?;
        self.eat_space();
        if self.bump() != Some('=') {
            return Err(self.fail("expected_key_equals_value"));
        }
        let value = self.parse_value()?;
        Ok((key, value, line))
    }

    /// Require end of line (or file) after one entry or header.
    fn end_of_line(&mut self) -> Result<(), TomlDiagnostic> {
        self.eat_space();
        match self.peek() {
            None | Some('\n') => Ok(()),
            Some('#') => {
                self.skip_comment();
                Ok(())
            }
            Some(_) => Err(self.fail("expected_end_of_line")),
        }
    }

    /// Skip spaces, newlines, and comments between entries.
    pub(crate) fn skip_trivia(&mut self) {
        loop {
            self.eat_space();
            match self.peek() {
                Some('#') => self.skip_comment(),
                Some('\n') => {
                    self.bump();
                }
                _ => return,
            }
        }
    }

    /// Skip horizontal whitespace (spaces, tabs, carriage returns).
    fn eat_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\r')) {
            self.bump();
        }
    }

    /// Skip a `#` comment up to (not consuming) the newline.
    fn skip_comment(&mut self) {
        while !matches!(self.peek(), None | Some('\n')) {
            self.bump();
        }
    }

    /// Consume `want` when it is next; report whether it matched.
    fn eat(&mut self, want: char) -> bool {
        if self.peek() == Some(want) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Peek the next character without consuming it.
    pub(crate) fn peek(&self) -> Option<char> {
        self.peek_at(0)
    }

    /// Peek the character `offset` ahead of the cursor.
    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    /// Whether the cursor reached the input end.
    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    /// Consume one character, tracking newlines.
    fn bump(&mut self) -> Option<char> {
        let next = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if next == '\n' {
            self.line = self.line.saturating_add(1);
        }
        Some(next)
    }

    /// Diagnostic at the current line for `problem`.
    fn fail(&self, problem: &str) -> TomlDiagnostic {
        TomlDiagnostic {
            line: self.line,
            problem: problem.to_owned(),
        }
    }
}
/// Inline-table parse value: scalars flatten, tables recurse.
pub(crate) enum InlineValue {
    /// Scalar or array leaf.
    Scalar(TomlValue),
    /// Nested `(dotted key, value, key line)` entries.
    Table(Vec<(Vec<String>, InlineValue, u32)>),
}

/// Whether `char` may appear in a bare key.
fn is_bare_char(char: char) -> bool {
    char.is_ascii_alphanumeric() || char == '_' || char == '-'
}

/// Whether `char` may appear in a bare scalar token.
fn is_bare_value_char(char: char) -> bool {
    char.is_ascii_alphanumeric() || matches!(char, '_' | '+' | '-' | '.' | ':')
}
