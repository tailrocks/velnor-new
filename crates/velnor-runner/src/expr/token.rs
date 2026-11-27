// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/lexer.rs

use super::number::parse_number;
use super::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    StartGroup,
    StartIndex,
    StartParameters,
    EndGroup,
    EndIndex,
    EndParameters,
    Separator,
    Dereference,
    Wildcard,
    LogicalOperator,
    Null,
    Boolean,
    Number,
    String,
    PropertyName,
    Function,
    NamedValue,
    Unexpected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Associativity {
    None,
    LeftToRight,
    RightToLeft,
}

#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) raw_value: String,
    pub(crate) index: usize,
    pub(crate) parsed_value: Option<Value>,
}

impl Token {
    pub(crate) fn new(
        kind: TokenKind,
        raw_value: String,
        index: usize,
        parsed_value: Option<Value>,
    ) -> Self {
        Self {
            kind,
            raw_value,
            index,
            parsed_value,
        }
    }

    pub(crate) fn unexpected(raw_value: String, index: usize) -> Self {
        Self::new(TokenKind::Unexpected, raw_value, index, None)
    }

    pub(crate) fn associativity(&self) -> Associativity {
        match self.kind {
            TokenKind::StartGroup => Associativity::None,
            TokenKind::LogicalOperator if self.raw_value == "!" => Associativity::RightToLeft,
            _ if self.is_operator() => Associativity::LeftToRight,
            _ => Associativity::None,
        }
    }

    pub(crate) fn is_operator(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::StartGroup
                | TokenKind::StartIndex
                | TokenKind::StartParameters
                | TokenKind::EndGroup
                | TokenKind::EndIndex
                | TokenKind::EndParameters
                | TokenKind::Separator
                | TokenKind::Dereference
                | TokenKind::LogicalOperator
        )
    }

    pub(crate) fn precedence(&self) -> i32 {
        match self.kind {
            TokenKind::StartGroup => 20,
            TokenKind::StartIndex | TokenKind::StartParameters | TokenKind::Dereference => 19,
            TokenKind::LogicalOperator => match self.raw_value.as_str() {
                "!" => 16,
                ">" | ">=" | "<" | "<=" => 11,
                "==" | "!=" => 10,
                "&&" => 6,
                "||" => 5,
                _ => 0,
            },
            TokenKind::EndGroup
            | TokenKind::EndIndex
            | TokenKind::EndParameters
            | TokenKind::Separator => 1,
            _ => 0,
        }
    }

    pub(crate) fn operand_count(&self) -> usize {
        match self.kind {
            TokenKind::StartIndex | TokenKind::Dereference => 2,
            TokenKind::LogicalOperator => match self.raw_value.as_str() {
                "!" => 1,
                ">" | ">=" | "<" | "<=" | "==" | "!=" | "&&" | "||" => 2,
                _ => 0,
            },
            _ => 0,
        }
    }
}

pub(crate) fn is_legal_keyword(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn is_token_boundary(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | ')' | ']' | ',' | '.' | '!' | '>' | '<' | '=' | '&' | '|'
    ) || c.is_whitespace()
}

pub(crate) fn number_token(raw: &str) -> Option<Value> {
    let parsed = parse_number(raw);
    (!parsed.is_nan()).then_some(Value::Number(parsed))
}
