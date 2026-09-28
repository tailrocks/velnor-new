//! Event-time `plan-v1` / `merge-v1` JSON entrypoints (schema 1).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    BaselineStatus, ContractError, ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision,
    Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner,
    RunnerSelection, Trust, WorkflowEvent, canonical_json_bytes, digest_b3, plan_id_for_run,
    validate_run_key,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::TaskGroup;

use crate::OrchestratorError;
use crate::cover::apply_baseline;
use crate::discover::Discovery;
use crate::merge::BaselineManifest;
use crate::prepare::prepare;
use crate::select::select_groups;
use crate::vectors::task_argv;

/// Schema version accepted by both internal entrypoints.
pub(crate) const SCHEMA: u32 = 1;

/// Maximum canonical `matrix.json` bytes; oversize errors, never truncates.
pub(crate) const MATRIX_BUDGET_BYTES: usize = 262_144;

/// `plan-v1` request: run scope plus optional repo root and generator.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Run key (`local` or `r<id>-a<attempt>`).
    run_key: String,
    /// Base commit or null.
    base: Option<String>,
    /// Head commit.
    head: String,
    /// Triggering event.
    event: WorkflowEvent,
    /// Repository root override; defaults to the resolved root.
    #[serde(default)]
    root: Option<PathBuf>,
    /// Generator identity override.
    #[serde(default)]
    generator: Option<PlanGenerator>,
    /// Trusted baseline evidence for coverage classification.
    #[serde(default)]
    baseline_manifest: Option<serde_json::Value>,
}

/// `plan-v1` response: schema plus plan and matrix copies.
#[derive(Debug, Serialize)]
struct PlanResponse {
    /// Response schema; always 1.
    schema: u32,
    /// Affected plan.
    plan: Plan,
    /// Matrix copy, byte-identical to the embedded matrix.
    matrix: PlanMatrix,
}

/// Compute the affected plan plus matrix for one event (schema-1 JSON).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests and
/// discovery, selection, or validation failures.
pub fn plan_internal(request_json: &str) -> Result<String, OrchestratorError> {
    let request: PlanRequest =
        serde_json::from_str(request_json).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
    check_schema(request.schema)?;
    validate_run_key(&request.run_key).map_err(internal_contract)?;
    if request.head.trim().is_empty() {
        return Err(internal("empty_head"));
    }
    let root = plan_root(request.root.as_deref())?;
    let prep = prepare(&root)?;
    let catalog = ToolCatalog::pinned();
    let mut warnings = Vec::new();
    let selected = select_groups(
        &prep.root,
        request.event,
        request.base.as_deref(),
        &request.head,
        &prep.discovery,
        &mut warnings,
    );
    let mut plan = build_plan(
        &request,
        &prep.discovery,
        &selected,
        &prep.runner_label,
        prep.runner_selection,
        &catalog,
        warnings,
    )?;
    let manifest = request.baseline_manifest.and_then(|value| {
        serde_json::from_value::<BaselineManifest>(value)
            .inspect_err(|_| {
                plan.warnings
                    .push("baseline_miss:malformed_manifest".to_owned());
            })
            .ok()
    });
    apply_baseline(
        &mut plan,
        request.event,
        &prep.default_branch,
        &prep.root,
        velnor_actions_workflow_renderer::render::WORKFLOW_PATH,
        &catalog,
        manifest,
    )?;
    plan.validate().map_err(internal_contract)?;
    check_matrix_budget(&plan.matrix)?;
    let response = PlanResponse {
        schema: SCHEMA,
        matrix: plan.matrix.clone(),
        plan,
    };
    serde_json::to_string(&response).map_err(|err| OrchestratorError::Internal {
        problem: format!("response_encode:{err}"),
    })
}

/// Reject a matrix whose canonical bytes exceed the budget.
fn check_matrix_budget(matrix: &PlanMatrix) -> Result<(), OrchestratorError> {
    let bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    if bytes.len() > MATRIX_BUDGET_BYTES {
        return Err(internal(&format!("matrix_budget_exceeded:{}", bytes.len())));
    }
    Ok(())
}

/// Reject any schema other than 1.
pub(crate) fn check_schema(schema: u32) -> Result<(), OrchestratorError> {
    if schema == SCHEMA {
        Ok(())
    } else {
        Err(internal(&format!("unsupported_schema:{schema}")))
    }
}

/// Build an internal error.
pub(crate) fn internal(problem: &str) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: problem.to_owned(),
    }
}

/// Map a contract error into an internal error.
#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a map_err fn"
)]
pub(crate) fn internal_contract(error: ContractError) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: error.to_string(),
    }
}

/// Repository root: explicit override or resolved from the current directory.
fn plan_root(override_root: Option<&Path>) -> Result<PathBuf, OrchestratorError> {
    if let Some(root) = override_root {
        return root
            .canonicalize()
            .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()));
    }
    let cwd = std::env::current_dir().map_err(|err| OrchestratorError::RootDiscovery {
        problem: err.to_string(),
    })?;
    crate::root::resolve_root(&cwd)
}

/// Build the validated plan from selected groups.
fn build_plan(
    request: &PlanRequest,
    discovery: &Discovery,
    selected: &[&TaskGroup],
    label: &str,
    selection: RunnerSelection,
    catalog: &ToolCatalog,
    warnings: Vec<String>,
) -> Result<Plan, OrchestratorError> {
    let mut obligations = Vec::with_capacity(selected.len());
    let mut entries = Vec::with_capacity(selected.len());
    let mut task_ids = Vec::with_capacity(selected.len());
    for group in selected {
        let (obligation, entry) = plan_group(group, &request.run_key, catalog)?;
        task_ids.push(group.task_id.clone());
        obligations.push(obligation);
        entries.push(entry);
    }
    obligations.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    task_ids.sort();
    let selected_ids: BTreeSet<&str> = selected
        .iter()
        .map(|group| group.package_id.as_str())
        .collect();
    Ok(Plan {
        schema: SCHEMA,
        run_key: request.run_key.clone(),
        plan_id: plan_id_for_run(&request.run_key).map_err(internal_contract)?,
        base: request.base.clone(),
        head: request.head.clone(),
        event: request.event,
        runner: PlanRunner {
            label: label.to_owned(),
            selection,
        },
        trust: match request.event {
            WorkflowEvent::PullRequest => Trust::Pr,
            WorkflowEvent::Push | WorkflowEvent::MergeGroup => Trust::Trusted,
        },
        baseline: PlanBaseline {
            status: BaselineStatus::Unavailable,
            base_commit: None,
            run_id: None,
            artifact_id: None,
            artifact_name: None,
            manifest_digest: None,
            reason: Some("baseline_lookup_deferred".to_owned()),
        },
        generator: request.generator.clone().unwrap_or_else(default_generator),
        packages: plan_packages(discovery, &selected_ids),
        obligations,
        matrix: PlanMatrix { include: entries },
        task_ids,
        warnings,
    })
}

/// Obligation plus matrix entry for one selected group.
fn plan_group(
    group: &TaskGroup,
    run_key: &str,
    catalog: &ToolCatalog,
) -> Result<(PlanObligation, MatrixEntry), OrchestratorError> {
    let toolchain = toolchain_id(group, catalog).map_err(internal_contract)?;
    let argv = task_argv(group, catalog)?;
    let task_digest = digest_of(&TaskDigestInputs {
        task_id: &group.task_id,
        argv: &argv,
        toolchain_id: &toolchain,
    })
    .map_err(internal_contract)?;
    let input_digest = digest_of(&InputDigestInputs {
        task_id: &group.task_id,
        manifest: &manifest_for_key(&group.manifest_key),
        package_id: &group.package_id,
        toolchain_id: &toolchain,
    })
    .map_err(internal_contract)?;
    let obligation = PlanObligation {
        task_id: group.task_id.clone(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest,
        input_digest: input_digest.clone(),
        baseline_proof: None,
    };
    let entry = MatrixEntry::derive(
        velnor_actions_rust::STACK_ID,
        &group.task_id,
        adapter_metadata(group),
        execute_ids(group),
        &input_digest,
        run_key,
    )
    .map_err(internal_contract)?;
    Ok((obligation, entry))
}

/// Toolchain identity digest for one group.
fn toolchain_id(group: &TaskGroup, catalog: &ToolCatalog) -> Result<String, ContractError> {
    let mut tools = vec![PinnedTool::Rust];
    if group.compile_driver == "mbx" {
        tools.push(PinnedTool::MrBoxington);
    }
    let specs = catalog.tool_specs(&tools);
    Ok(digest_b3(&canonical_json_bytes(&specs)?))
}

/// Digest of canonical bytes for a serializable input struct.
fn digest_of<T: Serialize>(inputs: &T) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(inputs)?))
}

/// Task-digest preimage fields.
#[derive(Debug, Serialize)]
struct TaskDigestInputs<'a> {
    /// Stable task ID.
    task_id: &'a str,
    /// Fixed argument vector.
    argv: &'a [String],
    /// Toolchain identity digest.
    toolchain_id: &'a str,
}

/// Input-digest preimage fields.
#[derive(Debug, Serialize)]
struct InputDigestInputs<'a> {
    /// Stable task ID.
    task_id: &'a str,
    /// Manifest path.
    manifest: &'a str,
    /// Cargo package ID.
    package_id: &'a str,
    /// Toolchain identity digest.
    toolchain_id: &'a str,
}

/// Manifest path for a manifest key.
fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}

/// Opaque adapter metadata for one matrix entry.
fn adapter_metadata(group: &TaskGroup) -> serde_json::Value {
    serde_json::json!({
        "package_id": group.package_id,
        "package_name": group.package_name,
        "manifest_key": group.manifest_key,
        "kind": group.kind.as_str(),
        "configuration": group.configuration,
        "target": group.target,
    })
}

/// Single executable obligation named by kind.
fn execute_ids(group: &TaskGroup) -> ExecuteTaskIds {
    ExecuteTaskIds {
        tasks: BTreeMap::from([(
            group.kind.as_str().to_owned(),
            ExecuteTaskRef::Single(group.task_id.clone()),
        )]),
    }
}

/// Default generator identity when the request omits it.
fn default_generator() -> PlanGenerator {
    PlanGenerator {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        sha256: "0".repeat(64),
    }
}

/// Complete package inventory with selection flags.
fn plan_packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut packages = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if !package.in_workspace || package.external {
                continue;
            }
            let is_selected = selected.contains(package.id.as_str());
            let mut tasks: Vec<String> = discovery
                .task_groups
                .iter()
                .filter(|group| group.package_id == package.id)
                .map(|group| group.task_id.clone())
                .collect();
            tasks.sort();
            packages.push(PlanPackage {
                package_id: package.id.clone(),
                name: package.name.clone(),
                manifest: package.manifest.clone(),
                selected: is_selected,
                reasons: vec![if is_selected {
                    "selected".to_owned()
                } else {
                    "not_affected".to_owned()
                }],
                tasks,
            });
        }
    }
    packages.sort_by(|left, right| left.package_id.cmp(&right.package_id));
    packages
}
