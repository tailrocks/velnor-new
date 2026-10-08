//! Provider-specific matrix payloads derived from the authoritative plan.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::canonical_json_str;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::ArtifactBuildOutput;

use super::{
    ArtifactBuildProvider, ArtifactBuildTaskPlan, artifact_name_for_run_key, canonical_plan_digest,
};
use crate::workflow::Plan;

/// Typed include list consumed by one provider's artifact build matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildMatrix {
    /// One exact plan task per matrix job.
    pub include: Vec<ArtifactBuildMatrixEntry>,
}

/// One immutable planned artifact-build task for a single provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildMatrixEntry {
    /// Exact full source commit.
    pub source_sha: String,
    /// Canonical digest of the validated plan.
    pub plan_digest: String,
    /// Selected provider fixed by this matrix's consuming job.
    pub provider: ArtifactBuildProvider,
    /// Stable task identity.
    pub task_id: String,
    /// Exact provider/run/task-scoped artifact name used by the upload action.
    pub artifact_name: String,
    /// Exact Mise task name from the plan.
    pub mise_task: String,
    /// Per-task job timeout, enforced by GitHub's matrix context.
    pub timeout_minutes: u16,
    /// Exact nonempty output declarations from the plan.
    pub outputs: Vec<ArtifactBuildOutput>,
}

/// Create the canonical matrix JSON for exactly one selected provider.
///
/// An empty matrix is valid when no artifact task is selected for the
/// provider; it creates no job. The complete task and provider inventory
/// remains in `plan.json`, which Required later uses as its authority.
/// # Errors
pub fn artifact_matrix_for_provider(
    plan: &Plan,
    provider: ArtifactBuildProvider,
) -> Result<String, ContractError> {
    plan.validate()?;
    let plan_digest = canonical_plan_digest(plan)?;
    let include = plan
        .artifact_tasks
        .iter()
        .filter(|item| item.providers.contains(&provider))
        .map(|item| matrix_entry(plan, &plan_digest, provider, item))
        .collect::<Result<Vec<_>, _>>()?;
    canonical_json_str(&ArtifactBuildMatrix { include })
}

fn matrix_entry(
    plan: &Plan,
    plan_digest: &str,
    provider: ArtifactBuildProvider,
    item: &ArtifactBuildTaskPlan,
) -> Result<ArtifactBuildMatrixEntry, ContractError> {
    Ok(ArtifactBuildMatrixEntry {
        source_sha: plan.head.clone(),
        plan_digest: plan_digest.to_owned(),
        provider,
        task_id: item.task.id.clone(),
        artifact_name: artifact_name_for_run_key(&plan.run_key, provider, &item.task.id)?,
        mise_task: item.task.mise_task.clone(),
        timeout_minutes: item.task.timeout_minutes,
        outputs: item.task.outputs.clone(),
    })
}
