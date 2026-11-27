// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

use super::facts::TrustedFacts;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Token(String);

impl Token {
    pub(crate) fn new(token: String) -> Self {
        Self(token)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Token(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    FetchMirror {
        token: Option<Token>,
    },
    RepairMirror {
        token: Option<Token>,
        window: NonZeroU64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Response {
    Hello(Box<TrustedFacts>),
    Done { notices: Vec<String> },
    Sound,
    Failed { message: String },
}
