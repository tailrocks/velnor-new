// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The plan request the qualification step reads: what `write-request` wrote.

use serde::Deserialize;
use serde_json::Value;

use super::strict;
use super::types::Dispatch;
use crate::error::CiError;
use crate::write_request::{SCHEMA, internal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Event {
    PullRequest,
    Push,
    MergeGroup,
    Local,
    Fork,
    Qualification,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    schema: u32,
    op: String,
    event: Event,
    #[serde(rename = "base", default)]
    _base: Option<String>,
    head: String,
    root: String,
    repository: String,
    qualification: Dispatch,
}

impl Request {
    pub fn parse(text: &str) -> Result<Self, CiError> {
        let value = strict::parse(text)?;
        serde_json::from_value(value).map_err(|_| internal("malformed_qualification_request"))
    }

    /// The checks that need nothing outside the request.
    pub fn check(&self) -> Result<(), CiError> {
        if self.schema != SCHEMA {
            return Err(internal(&format!("unsupported_schema:{}", self.schema)));
        }
        if self.op != "plan-v1" || self.event != Event::Qualification || self.root != "." {
            return Err(internal("qualification_request_mismatch"));
        }
        if self.repository != self.qualification.repository {
            return Err(internal("qualification_repository_mismatch"));
        }
        Ok(())
    }

    /// The dispatch context against the repository's default branch and the request's head.
    pub fn check_context(&self, default_branch: &str) -> Result<(), CiError> {
        self.qualification
            .validate_for(default_branch, &self.repository, &self.head)
    }

    /// Whether the dispatch the runner derives from its own context is the one in the request.
    #[must_use]
    pub fn matches_runner(&self, derived: Option<&Value>) -> bool {
        let same = serde_json::to_value(&self.qualification)
            .ok()
            .as_ref()
            .is_some_and(|context| derived == Some(context));
        same && self.repository == self.qualification.repository
    }

    pub(super) fn context(&self) -> &Dispatch {
        &self.qualification
    }

    pub(super) fn repository(&self) -> &str {
        &self.repository
    }
}
