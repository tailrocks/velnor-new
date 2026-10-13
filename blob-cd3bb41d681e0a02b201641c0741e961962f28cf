//! Parsed `plan-v1` request and its resolved checkout root.

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use velnor_actions_contract::{
    NamedCheckLane, QualificationDispatch, WorkflowEvent, parse_strict_json,
};

use super::resolve_run_key;
use crate::OrchestratorError;
use crate::internal::{PLAN_OP, check_schema, internal, internal_contract};

/// `plan-v1` request: run scope plus optional repo root.
///
/// The request carries no generator identity: the plan always names the
/// running binary, so a hand-written request cannot claim a release pin
/// for a source build. Unknown fields (including `generator`) reject.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanRequest {
    /// Request schema; must be 1.
    pub(crate) schema: u32,
    /// Consuming operation; must be `plan-v1` when present.
    #[serde(default)]
    pub(crate) op: Option<String>,
    /// Run key; empty derives from the GitHub environment.
    #[serde(default)]
    pub(crate) run_key: String,
    /// Base commit or null.
    pub(crate) base: Option<String>,
    /// Head commit.
    pub(crate) head: String,
    /// Triggering event.
    pub(crate) event: WorkflowEvent,
    /// Runner context for a protected hosted qualification dispatch.
    #[serde(default)]
    pub(crate) qualification: Option<QualificationDispatch>,
    /// Repository root override; defaults to the resolved root.
    #[serde(default)]
    pub(crate) root: Option<PathBuf>,
    /// Trusted baseline evidence for coverage classification.
    #[serde(default)]
    pub(crate) baseline_manifest: Option<serde_json::Value>,
    /// Runner-owned repository slug (`owner/repo`) for provenance.
    ///
    /// The request writer captures this from `GITHUB_REPOSITORY`; the
    /// planner never reads ambient env itself, keeping classification a
    /// function of request plus checkout. Absent means local run, where
    /// the git origin is the fallback.
    #[serde(default)]
    pub(crate) repository: Option<String>,
    /// Exact named-check job identities carried by the generated workflow.
    #[serde(default)]
    pub(crate) named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
}

/// Parse and validate one internal plan request and its checkout root.
pub(crate) fn checked_plan_request(
    request_json: &str,
) -> Result<(PlanRequest, PathBuf), OrchestratorError> {
    let envelope = parse_strict_json(request_json).map_err(internal_contract)?;
    let mut request: PlanRequest =
        serde_json::from_value(envelope).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
    check_schema(request.schema)?;
    if request.op.as_deref().is_some_and(|op| op != PLAN_OP) {
        return Err(internal("op_mismatch"));
    }
    request.run_key = resolve_run_key(Some(request.run_key.as_str()))?;
    if request.head.trim().is_empty() {
        return Err(internal("empty_head"));
    }
    let root = plan_root(request.root.as_deref())?;
    crate::select::verify_checkout(&root, request.event, &request.head)?;
    Ok((request, root))
}

/// Repository root: explicit override or resolved from the current directory.
fn plan_root(override_root: Option<&Path>) -> Result<PathBuf, OrchestratorError> {
    if let Some(root) = override_root {
        return root
            .canonicalize()
            .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()));
    }
    let cwd = env::current_dir().map_err(|err| OrchestratorError::RootDiscovery {
        problem: err.to_string(),
    })?;
    crate::root::resolve_root(&cwd)
}
