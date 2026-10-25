use crate::toml_scan::{TomlDiagnostic, TomlDoc, TomlScope, TomlValue, flatten_entry};

pub(crate) struct Parser {
    pub(crate) chars: Vec<char>,
    pub(crate) pos: usize,
    pub(crate) line: u32,
}

impl Parser {
    pub(crate) fn new(content: &str) -> Self {
        Self {
            chars: content.chars().collect(),
            pos: 0,
            line: 1,
        }
    }

    pub(crate) fn parse_header(
        &mut self,
        doc: &mut TomlDoc,
        scope: &mut TomlScope<'_>,
    ) -> Result<(Vec<String>, bool), TomlDiagnostic> {
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
        if scope.defined.contains(&path) {
            return Err(TomlDiagnostic {
                line,
                problem: "duplicate_key".to_owned(),
            });
        }
        for width in 1..path.len() {
            if scope.defined.contains(&path[..width]) {
                return Err(TomlDiagnostic {
                    line,
                    problem: "duplicate_key".to_owned(),
                });
            }
        }
        if array {
            if scope.tables.contains(&path) {
                return Err(TomlDiagnostic {
                    line,
                    problem: "duplicate_key".to_owned(),
                });
            }
            scope.array_tables.insert(path.clone());
        } else {
            if scope.tables.contains(&path) || scope.array_tables.contains(&path) {
                return Err(TomlDiagnostic {
                    line,
                    problem: "duplicate_key".to_owned(),
                });
            }
            scope.tables.insert(path.clone());
        }
        self.end_of_line()?;
        doc.sections.push((path.clone(), line));
        if array {
            doc.array_sections.push((path.clone(), line));
        }
        Ok((path, array))
    }

    pub(crate) fn parse_entry(
        &mut self,
        section: &[String],
        doc: &mut TomlDoc,
        scope: &mut TomlScope<'_>,
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
        path.extend(key.clone());
        flatten_entry(path, key, value, line, doc, scope)
    }

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

    fn eat_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\r')) {
            self.bump();
        }
    }

    fn skip_comment(&mut self) {
        while !matches!(self.peek(), None | Some('\n')) {
            self.bump();
        }
    }

    fn eat(&mut self, want: char) -> bool {
        if self.peek() == Some(want) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.peek_at(0)
    }

    pub(crate) fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    pub(crate) fn bump(&mut self) -> Option<char> {
        let next = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if next == '\n' {
            self.line = self.line.saturating_add(1);
        }
        Some(next)
    }

    pub(crate) fn fail(&self, problem: &str) -> TomlDiagnostic {
        TomlDiagnostic {
            line: self.line,
            problem: problem.to_owned(),
        }
    }
}
pub(crate) enum InlineValue {
    Scalar(TomlValue),
    Table(Vec<(Vec<String>, InlineValue, u32)>),
}

fn is_bare_char(char: char) -> bool {
    char.is_ascii_alphanumeric() || char == '_' || char == '-'
}

fn is_bare_value_char(char: char) -> bool {
    char.is_ascii_alphanumeric() || matches!(char, '_' | '+' | '-' | '.' | ':')
}
