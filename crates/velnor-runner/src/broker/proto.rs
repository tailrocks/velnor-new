// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! What crosses the sandbox boundary: one JSON message per datagram. A request
//! says what to write and never where or for whom; that comes from the facts.

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
    FetchMirror { token: Option<Token> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Response {
    Hello(Box<TrustedFacts>),
    Done,
    Failed { message: String },
}
