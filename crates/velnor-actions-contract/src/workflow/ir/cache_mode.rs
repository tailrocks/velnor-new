// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

use super::CACHE_MODE_PUSH_WRITE_EXPR;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
    Read,

    Write,

    WriteOnly,

    None,

    #[serde(untagged)]
    Expression(String),
}
impl CacheMode {
    #[must_use]
    pub fn explicit_read() -> Self {
        Self::Read
    }

    #[must_use]
    pub fn is_explicit_read(&self) -> bool {
        matches!(self, Self::Read)
    }

    #[must_use]
    pub fn grants_write(&self) -> bool {
        matches!(self, Self::Write | Self::WriteOnly | Self::Expression(_))
    }

    #[must_use]
    pub fn grants_unconditional_write(&self) -> bool {
        matches!(self, Self::Write | Self::WriteOnly)
    }
}

impl CacheMode {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::WriteOnly => "write-only",
            Self::None => "none",
            Self::Expression(expr) => expr.as_str(),
        }
    }

    #[must_use]
    pub fn push_write_expression() -> Self {
        Self::Expression(CACHE_MODE_PUSH_WRITE_EXPR.to_owned())
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let mode = match value {
            "read" => Self::Read,
            "write" => Self::Write,
            "write-only" => Self::WriteOnly,
            "none" => Self::None,
            expr if expr.starts_with("${{") && expr.ends_with("}}") => {
                Self::Expression(expr.to_owned())
            }
            _ => {
                return Err(ContractError::identity(
                    "cache_mode",
                    format!("invalid_cache_mode:{value}"),
                ));
            }
        };
        mode.validate("cache_mode")?;
        Ok(mode)
    }

    pub fn validate(&self, field: &'static str) -> Result<(), ContractError> {
        if let Self::Expression(expr) = self {
            let inner = expr
                .strip_prefix("${{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .map(str::trim);
            if inner.is_none_or(str::is_empty) || expr.chars().any(char::is_control) {
                return Err(ContractError::identity(field, "bad_cache_mode_expression"));
            }
        }
        Ok(())
    }
}
