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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanRequest {
    pub(crate) schema: u32,
    #[serde(default)]
    pub(crate) op: Option<String>,
    #[serde(default)]
    pub(crate) run_key: String,
    pub(crate) base: Option<String>,
    pub(crate) head: String,
    pub(crate) event: WorkflowEvent,
    #[serde(default)]
    pub(crate) qualification: Option<QualificationDispatch>,
    #[serde(default)]
    pub(crate) root: Option<PathBuf>,
    #[serde(default)]
    pub(crate) repository: Option<String>,
    #[serde(default)]
    pub(crate) named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
}

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
