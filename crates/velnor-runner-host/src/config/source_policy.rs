// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePolicyMode {
    Repository,

    BranchRestricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEvent {
    PullRequest,

    Push,

    MergeGroup,

    Schedule,

    WorkflowDispatch,

    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequestRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,

    pub base_repository_id: i64,

    pub base_branch: String,

    pub head_repository_id: i64,

    pub head_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePolicy {
    pub mode: SourcePolicyMode,

    pub branches: Vec<String>,

    pub tag_prefixes: Vec<String>,

    pub events: Vec<WorkflowEvent>,

    pub pull_request_rules: Vec<PullRequestRule>,
}

impl SourcePolicy {
    pub fn try_new(
        mode: SourcePolicyMode,
        branches: Vec<String>,
        tag_prefixes: Vec<String>,
        events: Vec<WorkflowEvent>,
        pull_request_rules: Vec<PullRequestRule>,
    ) -> Result<Self, HostError> {
        let mut policy = Self {
            mode,
            branches,
            tag_prefixes,
            events,
            pull_request_rules,
        };
        policy.canonicalize();
        policy.validate()?;
        Ok(policy)
    }

    pub(crate) fn canonicalize(&mut self) {
        self.branches.sort();
        self.tag_prefixes.sort();
        self.events.sort();
        self.pull_request_rules.sort();
    }

    pub(crate) fn validate(&self) -> Result<(), HostError> {
        if self.events.is_empty()
            || self.branches.len() > 128
            || self.tag_prefixes.len() > 64
            || self.pull_request_rules.len() > 128
            || !unique(&self.branches)
            || !unique(&self.tag_prefixes)
            || !unique(&self.events)
            || !unique(&self.pull_request_rules)
            || self.branches.iter().any(|branch| !valid_ref(branch))
            || self.tag_prefixes.iter().any(|prefix| !valid_ref(prefix))
            || self.pull_request_rules.iter().any(|rule| !valid_rule(rule))
        {
            return Err(HostError::Config);
        }
        let has_event = |event| self.events.contains(&event);
        if self.mode == SourcePolicyMode::Repository {
            return if self.branches.is_empty()
                && self.tag_prefixes.is_empty()
                && self.pull_request_rules.is_empty()
                && !has_event(WorkflowEvent::PullRequest)
            {
                Ok(())
            } else {
                Err(HostError::Config)
            };
        }
        let needs_branch = [WorkflowEvent::MergeGroup, WorkflowEvent::Schedule]
            .into_iter()
            .any(has_event);
        if (needs_branch && self.branches.is_empty())
            || (has_event(WorkflowEvent::Push)
                && self.branches.is_empty()
                && self.tag_prefixes.is_empty())
            || (has_event(WorkflowEvent::WorkflowDispatch)
                && self.branches.is_empty()
                && self.tag_prefixes.is_empty())
            || (has_event(WorkflowEvent::PullRequest) == self.pull_request_rules.is_empty())
            || (!self.tag_prefixes.is_empty()
                && !has_event(WorkflowEvent::Push)
                && !has_event(WorkflowEvent::Release)
                && !has_event(WorkflowEvent::WorkflowDispatch))
            || (has_event(WorkflowEvent::Release) && self.tag_prefixes.is_empty())
        {
            return Err(HostError::Config);
        }
        Ok(())
    }
}

fn valid_rule(rule: &PullRequestRule) -> bool {
    rule.number.is_none_or(|number| number > 0)
        && rule.base_repository_id > 0
        && rule.head_repository_id > 0
        && valid_ref(&rule.base_branch)
        && valid_ref(&rule.head_branch)
}

fn valid_ref(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.starts_with('-')
        || value.contains("..")
        || value.contains("//")
        || value.contains("@{")
        || value == "@"
    {
        return false;
    }
    value.split('/').all(|part| {
        !part.is_empty()
            && !part.starts_with('.')
            && !part.as_bytes().ends_with(b".lock")
            && part.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'/')
            })
    })
}

fn unique<T: Eq + std::hash::Hash>(values: &[T]) -> bool {
    values.iter().collect::<HashSet<_>>().len() == values.len()
}
