use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::HostError;

/// Scope of one connection's authenticated source rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePolicyMode {
    /// Allow listed events from the bound repository.
    Repository,
    /// Require configured branch, tag, or pull request rules.
    BranchRestricted,
}

/// GitHub event allowed by one repository connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEvent {
    /// A pull request validation run.
    PullRequest,
    /// A branch or tag push.
    Push,
    /// A merge queue run.
    MergeGroup,
    /// A scheduled run.
    Schedule,
    /// A manually requested run.
    WorkflowDispatch,
    /// A published release run.
    Release,
}

/// One allowed pull request validation context.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequestRule {
    /// Exact pull request number. `None` matches any number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// Required base repository ID.
    pub base_repository_id: i64,
    /// Required base branch.
    pub base_branch: String,
    /// Required head repository ID.
    pub head_repository_id: i64,
    /// Required head branch.
    pub head_branch: String,
}

/// Trusted repository refs and events for one connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePolicy {
    /// Required source policy scope.
    pub mode: SourcePolicyMode,
    /// Exact branch refs allowed for branch events.
    pub branches: Vec<String>,
    /// Allowed tag prefixes. An empty list rejects all tag refs.
    pub tag_prefixes: Vec<String>,
    /// GitHub events allowed for this connection.
    pub events: Vec<WorkflowEvent>,
    /// Pull request contexts allowed for this connection.
    pub pull_request_rules: Vec<PullRequestRule>,
}

impl SourcePolicy {
    /// Create a sorted source policy.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when a ref or event rule is invalid.
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
            {
                Ok(())
            } else {
                Err(HostError::Config)
            };
        }
        let needs_branch = [
            WorkflowEvent::MergeGroup,
            WorkflowEvent::Schedule,
            WorkflowEvent::WorkflowDispatch,
        ]
        .into_iter()
        .any(has_event);
        if (needs_branch && self.branches.is_empty())
            || (has_event(WorkflowEvent::Push)
                && self.branches.is_empty()
                && self.tag_prefixes.is_empty())
            || (has_event(WorkflowEvent::PullRequest) == self.pull_request_rules.is_empty())
            || (!self.tag_prefixes.is_empty()
                && !has_event(WorkflowEvent::Push)
                && !has_event(WorkflowEvent::Release))
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
