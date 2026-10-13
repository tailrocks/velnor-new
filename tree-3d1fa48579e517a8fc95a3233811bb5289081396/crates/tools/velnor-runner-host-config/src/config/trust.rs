//! Structured per-offer workflow trust policy records.

use serde::Deserialize;

/// One exact per-offer workflow trust tuple from the protected host config.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobTrustRule {
    /// Full root workflow reference: `<repository>/<REST path>`.
    pub workflow_ref: String,
    /// Exact protocol `jobWorkflowRef` allowed by this rule.
    pub job_workflow_ref: String,
    /// Literal workflow-run REST `path`, including its `@ref` suffix.
    pub workflow_path: String,
    /// Exact Actions event name.
    pub event: String,
    /// Exact workflow-run `head_branch` value.
    pub head_branch: String,
    /// Ordered exact REST reusable-workflow chain.
    #[serde(default)]
    pub referenced_workflows: Vec<ReusableWorkflowRule>,
}

/// One exact reusable workflow in a per-offer trust rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReusableWorkflowRule {
    /// Literal REST `referenced_workflows[].path` value.
    pub path: String,
    /// Literal REST `referenced_workflows[].ref` value.
    pub git_ref: String,
    /// Exact commit SHA reported by the REST API.
    pub sha: String,
}
