// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/mod.rs

use super::parser::{MAX_DEPTH, MAX_LENGTH};
use super::token::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    ExceededMaxDepth,
    ExceededMaxLength,
    TooFewParameters,
    TooManyParameters,
    UnexpectedEndOfExpression,
    UnexpectedSymbol,
    UnsupportedFunction,
    UnsupportedContext,
    Internal,
}

impl ParseErrorKind {
    fn description(self) -> String {
        match self {
            ParseErrorKind::ExceededMaxDepth => {
                format!("Exceeded max expression depth {MAX_DEPTH}")
            }
            ParseErrorKind::ExceededMaxLength => {
                format!("Exceeded max expression length {MAX_LENGTH}")
            }
            ParseErrorKind::TooFewParameters => "Too few parameters supplied".to_string(),
            ParseErrorKind::TooManyParameters => "Too many parameters supplied".to_string(),
            ParseErrorKind::UnexpectedEndOfExpression => "Unexpected end of expression".to_string(),
            ParseErrorKind::UnexpectedSymbol => "Unexpected symbol".to_string(),
            ParseErrorKind::UnsupportedFunction => "unsupported expression function".to_string(),
            ParseErrorKind::UnsupportedContext => "unsupported expression context".to_string(),
            ParseErrorKind::Internal => "Invalid expression".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ParseError {
    pub kind: ParseErrorKind,
    message: String,
}

impl ParseError {
    pub(crate) fn with_token(kind: ParseErrorKind, token: &Token, expression: &str) -> Self {
        let message = match kind {
            ParseErrorKind::UnsupportedFunction | ParseErrorKind::UnsupportedContext => {
                format!("{} {}", kind.description(), token.raw_value)
            }
            _ => format!(
                "{}: '{}'. Located at position {} within expression: {expression}",
                kind.description(),
                token.raw_value,
                token.index + 1
            ),
        };
        Self { kind, message }
    }

    pub(crate) fn unsupported_function(name: &str) -> Self {
        let kind = ParseErrorKind::UnsupportedFunction;
        Self {
            kind,
            message: format!("{} {name}", kind.description()),
        }
    }

    pub(crate) fn without_token(kind: ParseErrorKind) -> Self {
        Self {
            kind,
            message: kind.description(),
        }
    }

    pub(crate) fn internal(expression: &str, detail: &str) -> Self {
        Self {
            kind: ParseErrorKind::Internal,
            message: format!("Invalid expression: {detail}. Expression: {expression}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExprError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("{0}")]
    Evaluation(String),
}

impl ExprError {
    pub(crate) fn evaluation(message: impl Into<String>) -> Self {
        ExprError::Evaluation(message.into())
    }
}
