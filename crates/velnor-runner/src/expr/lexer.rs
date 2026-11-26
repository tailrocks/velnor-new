// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/lexer.rs

use super::token::{Token, TokenKind, is_legal_keyword, is_token_boundary, number_token};
use super::value::Value;

const AFTER_OPERAND: &[Option<TokenKind>] = &[
    Some(TokenKind::EndGroup),
    Some(TokenKind::EndParameters),
    Some(TokenKind::EndIndex),
    Some(TokenKind::Wildcard),
    Some(TokenKind::Null),
    Some(TokenKind::Boolean),
    Some(TokenKind::Number),
    Some(TokenKind::String),
    Some(TokenKind::PropertyName),
    Some(TokenKind::NamedValue),
];

const BEFORE_OPERAND: &[Option<TokenKind>] = &[
    None,
    Some(TokenKind::Separator),
    Some(TokenKind::StartGroup),
    Some(TokenKind::StartParameters),
    Some(TokenKind::StartIndex),
    Some(TokenKind::LogicalOperator),
];

const BEFORE_INDEX: &[Option<TokenKind>] = &[
    Some(TokenKind::EndGroup),
    Some(TokenKind::EndParameters),
    Some(TokenKind::EndIndex),
    Some(TokenKind::Wildcard),
    Some(TokenKind::PropertyName),
    Some(TokenKind::NamedValue),
];

pub(crate) struct LexicalAnalyzer {
    expression: Vec<char>,
    index: usize,
    last_token: Option<Token>,
    unclosed_tokens: Vec<Token>,
}

impl LexicalAnalyzer {
    pub(crate) fn new(expression: &str) -> Self {
        Self {
            expression: expression.chars().collect(),
            index: 0,
            last_token: None,
            unclosed_tokens: Vec::new(),
        }
    }

    pub(crate) fn has_unclosed_tokens(&self) -> bool {
        !self.unclosed_tokens.is_empty()
    }

    fn peek_unclosed(&self) -> Option<TokenKind> {
        self.unclosed_tokens.last().map(|token| token.kind)
    }

    fn last_kind(&self) -> Option<TokenKind> {
        self.last_token.as_ref().map(|token| token.kind)
    }

    fn char_at(&self, index: usize) -> Option<char> {
        self.expression.get(index).copied()
    }

    fn substring(&self, start: usize, end: usize) -> String {
        self.expression
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .collect()
    }

    fn single(&mut self, kind: TokenKind, c: char) -> Token {
        let index = self.index;
        self.index += 1;
        self.create_token(kind, c.to_string(), index, None)
    }

    pub(crate) fn next_token(&mut self) -> Option<Token> {
        while self.char_at(self.index).is_some_and(char::is_whitespace) {
            self.index += 1;
        }
        let c = self.char_at(self.index)?;
        let token = match c {
            '(' => {
                let kind = if self.last_kind() == Some(TokenKind::Function) {
                    TokenKind::StartParameters
                } else {
                    TokenKind::StartGroup
                };
                self.single(kind, c)
            }
            '[' => self.single(TokenKind::StartIndex, c),
            ')' => {
                let kind = if self.peek_unclosed() == Some(TokenKind::StartParameters) {
                    TokenKind::EndParameters
                } else {
                    TokenKind::EndGroup
                };
                self.single(kind, c)
            }
            ']' => self.single(TokenKind::EndIndex, c),
            ',' => self.single(TokenKind::Separator, c),
            '*' => self.single(TokenKind::Wildcard, c),
            '\'' => self.read_string_token(),
            '!' | '>' | '<' | '=' | '&' | '|' => self.read_operator(),
            '.' if BEFORE_OPERAND.contains(&self.last_kind()) => self.read_number_token(),
            '.' => self.single(TokenKind::Dereference, c),
            '-' | '+' | '0'..='9' => self.read_number_token(),
            _ => self.read_keyword_token(),
        };
        self.last_token = Some(token.clone());
        Some(token)
    }

    fn read_number_token(&mut self) -> Token {
        let start_index = self.index;
        loop {
            self.index += 1;
            match self.char_at(self.index) {
                Some(c) if !is_token_boundary(c) || c == '.' => {}
                _ => break,
            }
        }
        let raw = self.substring(start_index, self.index);
        match number_token(&raw) {
            Some(value) => self.create_token(TokenKind::Number, raw, start_index, Some(value)),
            None => self.create_token(TokenKind::Unexpected, raw, start_index, None),
        }
    }

    fn read_keyword_token(&mut self) -> Token {
        let start_index = self.index;
        self.index += 1;
        while self
            .char_at(self.index)
            .is_some_and(|c| !is_token_boundary(c))
        {
            self.index += 1;
        }
        let raw = self.substring(start_index, self.index);
        if !is_legal_keyword(&raw) {
            return self.create_token(TokenKind::Unexpected, raw, start_index, None);
        }
        if self.last_kind() == Some(TokenKind::Dereference) {
            return self.create_token(TokenKind::PropertyName, raw, start_index, None);
        }
        let literal = match raw.as_str() {
            "null" => Some((TokenKind::Null, Value::Null)),
            "true" => Some((TokenKind::Boolean, Value::Boolean(true))),
            "false" => Some((TokenKind::Boolean, Value::Boolean(false))),
            "NaN" => Some((TokenKind::Number, Value::Number(f64::NAN))),
            "Infinity" => Some((TokenKind::Number, Value::Number(f64::INFINITY))),
            _ => None,
        };
        if let Some((kind, value)) = literal {
            return self.create_token(kind, raw, start_index, Some(value));
        }
        let mut lookahead = self.index;
        while self.char_at(lookahead).is_some_and(char::is_whitespace) {
            lookahead += 1;
        }
        let kind = if self.char_at(lookahead) == Some('(') {
            TokenKind::Function
        } else {
            TokenKind::NamedValue
        };
        self.create_token(kind, raw, start_index, None)
    }

    fn read_string_token(&mut self) -> Token {
        let start_index = self.index;
        let mut value = String::new();
        let mut closed = false;
        self.index += 1;
        while let Some(c) = self.char_at(self.index) {
            self.index += 1;
            if c == '\'' {
                if self.char_at(self.index) != Some('\'') {
                    closed = true;
                    break;
                }
                self.index += 1;
            }
            value.push(c);
        }
        let raw = self.substring(start_index, self.index);
        if closed {
            self.create_token(
                TokenKind::String,
                raw,
                start_index,
                Some(Value::String(value)),
            )
        } else {
            self.create_token(TokenKind::Unexpected, raw, start_index, None)
        }
    }

    fn read_operator(&mut self) -> Token {
        let start_index = self.index;
        self.index += 1;
        if self.index < self.expression.len() {
            self.index += 1;
            let raw = self.substring(start_index, start_index + 2);
            if matches!(raw.as_str(), "!=" | ">=" | "<=" | "==" | "&&" | "||") {
                return self.create_token(TokenKind::LogicalOperator, raw, start_index, None);
            }
            self.index -= 1;
        }
        let raw = self.substring(start_index, start_index + 1);
        if matches!(raw.as_str(), "!" | ">" | "<") {
            return self.create_token(TokenKind::LogicalOperator, raw, start_index, None);
        }
        while self
            .char_at(self.index)
            .is_some_and(|c| !is_token_boundary(c))
        {
            self.index += 1;
        }
        let raw = self.substring(start_index, self.index);
        self.create_token(TokenKind::Unexpected, raw, start_index, None)
    }

    fn follows(&self, allowed: &[Option<TokenKind>]) -> bool {
        allowed.contains(&self.last_kind())
    }

    fn may_follow_last_token(&self, kind: TokenKind, raw_value: &str) -> bool {
        match kind {
            TokenKind::StartIndex | TokenKind::Dereference => self.follows(BEFORE_INDEX),
            TokenKind::StartParameters => self.follows(&[Some(TokenKind::Function)]),
            TokenKind::EndGroup | TokenKind::EndIndex | TokenKind::Separator => {
                self.follows(AFTER_OPERAND)
            }
            TokenKind::EndParameters => {
                self.follows(&[Some(TokenKind::StartParameters)]) || self.follows(AFTER_OPERAND)
            }
            TokenKind::Wildcard => {
                self.follows(&[Some(TokenKind::StartIndex), Some(TokenKind::Dereference)])
            }
            TokenKind::LogicalOperator if raw_value == "!" => self.follows(BEFORE_OPERAND),
            TokenKind::LogicalOperator => self.follows(AFTER_OPERAND),
            TokenKind::StartGroup
            | TokenKind::Null
            | TokenKind::Boolean
            | TokenKind::Number
            | TokenKind::String
            | TokenKind::Function
            | TokenKind::NamedValue => self.follows(BEFORE_OPERAND),
            TokenKind::PropertyName => self.follows(&[Some(TokenKind::Dereference)]),
            TokenKind::Unexpected => false,
        }
    }

    fn close_unclosed(&mut self, opener: TokenKind) -> bool {
        if self.peek_unclosed() != Some(opener) {
            return false;
        }
        self.unclosed_tokens.pop();
        true
    }

    fn create_token(
        &mut self,
        kind: TokenKind,
        raw_value: String,
        index: usize,
        parsed_value: Option<Value>,
    ) -> Token {
        if !self.may_follow_last_token(kind, &raw_value) {
            return Token::unexpected(raw_value, index);
        }
        let token = Token::new(kind, raw_value.clone(), index, parsed_value);
        let balanced = match kind {
            TokenKind::StartGroup | TokenKind::StartIndex | TokenKind::StartParameters => {
                self.unclosed_tokens.push(token.clone());
                true
            }
            TokenKind::EndGroup => self.close_unclosed(TokenKind::StartGroup),
            TokenKind::EndIndex => self.close_unclosed(TokenKind::StartIndex),
            TokenKind::EndParameters => self.close_unclosed(TokenKind::StartParameters),
            TokenKind::Separator => self.peek_unclosed() == Some(TokenKind::StartParameters),
            _ => true,
        };
        if balanced {
            token
        } else {
            Token::unexpected(raw_value, index)
        }
    }
}
