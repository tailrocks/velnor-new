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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Window {
    Last(NonZeroU64),
    All,
}

impl Window {
    pub(crate) const COMMIT: Self = Self::Last(NonZeroU64::MIN);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailedStep {
    ReadTree,
    CheckoutIndex,
    Other,
}

impl FailedStep {
    pub(crate) fn reads_only_the_commit(self) -> bool {
        matches!(self, Self::ReadTree | Self::CheckoutIndex)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    FetchMirror {
        token: Option<Token>,
    },
    FetchTrusted {
        token: Option<Token>,
    },
    RepairMirror {
        token: Option<Token>,
        window: Window,
        failed: FailedStep,
    },
    BeginCache {
        key: String,
        version: String,
    },
    SealCache {
        bytes: u64,
    },
    AbortCache,
    TouchCache {
        scope: String,
        key: String,
        version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Response {
    Hello(Box<TrustedFacts>),
    Done { notices: Vec<String> },
    Sound,
    Ready,
    Published { scope: String },
    AlreadyExists,
    Denied(Denied),
    Failed { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Denied {
    ForkPullRequest,
    ReadOnlyEvent,
}

impl std::fmt::Display for Denied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ForkPullRequest => "fork pull request",
            Self::ReadOnlyEvent => "event without write access",
        })
    }
}
