//! Generation-only source transport binding; whole-workflow qualification precedes emission.
use crate::OrchestratorError;
use crate::release_emit::release_steps::{self, JobInputs, reconcile};
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation, Step, StepKind};
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseWorkflowSpec};

#[path = "release_source_artifact_runtime.rs"]
mod runtime;

const SOURCE_JOB: &str = "release-source-snapshot";

/// Nonserialized compiled transport binding, never authenticated runtime content.
/// Fields come only from fresh source-owner and graph factories, not candidate IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompiledSourceArtifactInput {
    source_owner: CompiledSourceHelper,
    source_job: ReleaseJobSpec,
    environment: BTreeMap<String, String>,
    repository: String,
    source_sha: String,
    branch: String,
}

impl CompiledSourceArtifactInput {
    /// Source-only leaf: this never calls the whole spec or Package/Verify factories.
    /// SDK/source-owner failures propagate before a transport binding is returned.
    pub(crate) fn compile(inputs: &JobInputs<'_>) -> Result<Self, OrchestratorError> {
        let source_owner = reconcile::proof_record(inputs, reconcile::ProofKind::SourceSnapshot)?;
        let source_job = release_steps::source_snapshot_job(inputs)?;
        source_owner.validate_binding()?;
        if source_owner.invocation().descriptor().operation()
            != SourceBoundOperation::RustReleaseSourceSnapshot
            || !source_owner.invocation().args().is_empty()
            || !source_owner.github_output()
            || source_owner.execution_recipe().is_none()
        {
            return Err(invalid("source_owner_binding"));
        }
        let matching = source_job
            .steps
            .iter()
            .filter(|step| {
                step.id.as_ref().is_some_and(|id| id.as_str() == SOURCE_JOB)
                    && matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env }
                if invocation == source_owner.invocation() && env == source_owner.environment())
            })
            .count();
        if matching != 1 {
            return Err(invalid("source_owner_binding"));
        }
        Ok(Self {
            source_owner,
            source_job,
            environment: source_input_environment(),
            repository: inputs.repository.to_owned(),
            source_sha: inputs.sha.to_owned(),
            branch: inputs.branch.to_owned(),
        })
    }

    pub(crate) fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Exact-ID transport for this compiled source owner; no cross-run credentials.
    pub(crate) fn download_step(&self) -> Result<Step, OrchestratorError> {
        let artifact_id = self
            .environment
            .get("RELEASE_SOURCE_SNAPSHOT_ARTIFACT_ID")
            .ok_or_else(|| invalid("source_artifact_id_binding"))?;
        velnor_actions_workflow_renderer::steps::action_step(
            "Download source snapshot",
            velnor_actions_workflow_renderer::steps::DOWNLOAD_ARTIFACT_USES,
            BTreeMap::from([
                ("artifact-ids".to_owned(), artifact_id.clone()),
                (
                    "path".to_owned(),
                    "${{ runner.temp }}/velnor/release-source-input".to_owned(),
                ),
            ]),
        )
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })
    }

    /// Code is usable only as part of the qualified owner's complete sealed closure.
    pub(crate) fn runtime_source(&self) -> Result<String, OrchestratorError> {
        runtime::source(self)
    }
}

/// Sole final boundary: compare the entire spec with independent fresh reconstruction.
/// Never called inside transport/helper factories: construction remains acyclic.
pub(crate) fn validate_workflow(
    inputs: &JobInputs<'_>,
    candidate: &ReleaseWorkflowSpec,
) -> Result<(), OrchestratorError> {
    let binding = CompiledSourceArtifactInput::compile(inputs)?;
    if candidate.jobs.get(SOURCE_JOB) != Some(&binding.source_job) {
        return Err(invalid("compiled_source_job_mismatch"));
    }
    let expected = crate::release_emit::release_workflow_spec::compile(inputs)?;
    if candidate != &expected {
        return Err(invalid("compiled_workflow_mismatch"));
    }
    Ok(())
}

fn needs(output: &str) -> String {
    format!("${{{{ needs.{SOURCE_JOB}.outputs.{output} }}}}")
}

/// Fixed environment template only; not a compiled input capability.
fn source_input_environment() -> BTreeMap<String, String> {
    let mut environment: BTreeMap<String, String> = [
        (
            "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_ID",
            "source-snapshot-artifact-id",
        ),
        (
            "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_DIGEST",
            "source-snapshot-artifact-digest",
        ),
        (
            "RELEASE_SOURCE_SNAPSHOT_BLOB_SHA256",
            "source-snapshot-blob-sha256",
        ),
        ("RELEASE_SOURCE_COMMIT_SHA", "source-commit-sha"),
        ("RELEASE_SOURCE_TREE_SHA", "source-tree-sha"),
    ]
    .into_iter()
    .map(|(key, output)| (key.to_owned(), needs(output)))
    .collect();
    for key in ["GITHUB_REF", "GITHUB_WORKFLOW_REF", "GITHUB_WORKFLOW_SHA"] {
        let context = match key {
            "GITHUB_REF" => "ref",
            "GITHUB_WORKFLOW_REF" => "workflow_ref",
            _ => "workflow_sha",
        };
        environment.insert(key.to_owned(), format!("${{{{ github.{context} }}}}"));
    }
    environment.insert("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned());
    environment
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("release_source_artifact_input:{reason}"),
    }
}

#[cfg(test)]
#[path = "release_source_artifact_input_tests.rs"]
mod tests;
