//! Generation-only prepared-artifact transport binding.

use std::collections::BTreeMap;

use crate::OrchestratorError;
use crate::release_emit::release_steps::{self, JobInputs, reconcile};
use velnor_actions_contract::workflow::outputs::{ActionOutput, validate_job_outputs};
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation, StepKind};
use velnor_actions_workflow_renderer::release_jobs::ReleaseJobSpec;
use velnor_actions_workflow_renderer::release_jobs::{ReleaseRole, ReleaseWorkflowSpec};

#[path = "release_prepared_artifact_runtime.rs"]
mod runtime;

const PACKAGE_JOB: &str = "release-package";
const SOURCE_JOB: &str = "release-source-snapshot";

/// Nonserialized binding for the genuine prepared-package producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompiledPreparedArtifactInput {
    prepared_owner: CompiledSourceHelper,
    prepared_job: ReleaseJobSpec,
    environment: BTreeMap<String, String>,
    repository: String,
    source_sha: String,
    branch: String,
    policy: String,
}

impl CompiledPreparedArtifactInput {
    /// Reconstruct the prepared owner and its complete producer job.
    ///
    /// This leaf never accepts a candidate job or a caller-created helper.
    /// # Errors
    /// Fails closed when either fresh owner reconstruction or typed outputs drift.
    pub(super) fn compile(inputs: &JobInputs<'_>) -> Result<Self, OrchestratorError> {
        let prepared_owner =
            reconcile::proof_record(inputs, reconcile::ProofKind::PreparedPackage)?;
        let prepared_job = release_steps::prepared_package_job(inputs)?;
        prepared_owner.validate_binding()?;
        validate_prepared_owner(&prepared_owner)?;
        validate_prepared_job(&prepared_job, &prepared_owner)?;
        let policy = inputs.reconciliation.serialized()?;
        Ok(Self {
            prepared_owner,
            prepared_job,
            environment: environment(&policy),
            repository: inputs.repository.to_owned(),
            source_sha: inputs.sha.to_owned(),
            branch: inputs.branch.to_owned(),
            policy,
        })
    }

    /// Exact environment consumed by the prepared-artifact loader.
    #[must_use]
    pub(super) fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Emit the override that activates the genuine prepared input loader.
    /// # Errors
    /// Fails closed if the compiled identity cannot be encoded.
    pub(super) fn runtime_source(&self) -> Result<String, OrchestratorError> {
        runtime::source(self)
    }
}

fn validate_prepared_owner(owner: &CompiledSourceHelper) -> Result<(), OrchestratorError> {
    let invocation = owner.invocation();
    if invocation.descriptor().operation() != SourceBoundOperation::RustReleasePreparedPackage
        || !invocation.args().is_empty()
        || !owner.github_output()
        || owner.execution_recipe().is_none()
    {
        return Err(invalid("prepared_owner_binding"));
    }
    Ok(())
}

fn validate_prepared_job(
    job: &ReleaseJobSpec,
    owner: &CompiledSourceHelper,
) -> Result<String, OrchestratorError> {
    job.validate_shape(PACKAGE_JOB)?;
    validate_job_outputs(&job.outputs, &job.steps)?;
    if job.role.job_id() != PACKAGE_JOB
        || job.role != ReleaseRole::PackagePreparedAnonymous
        || job.needs.len() != 1
        || job.needs.first().map(String::as_str) != Some(SOURCE_JOB)
    {
        return Err(invalid("prepared_job_graph"));
    }
    let helper_step_id = prepared_helper_step_id(job, owner)?;
    validate_outputs(job, &helper_step_id)?;
    Ok(helper_step_id)
}

/// Compare the freshly reconstructed prepared producer with a candidate job.
///
/// This leaf does not compile or admit the surrounding workflow.
/// # Errors
/// Fails closed when the candidate release-package job differs from its owner.
pub(crate) fn validate_workflow(
    inputs: &JobInputs<'_>,
    candidate: &ReleaseWorkflowSpec,
) -> Result<(), OrchestratorError> {
    let binding = CompiledPreparedArtifactInput::compile(inputs)?;
    if candidate.jobs.get(PACKAGE_JOB) != Some(&binding.prepared_job) {
        return Err(invalid("compiled_prepared_job_mismatch"));
    }
    Ok(())
}

fn prepared_helper_step_id(
    job: &ReleaseJobSpec,
    owner: &CompiledSourceHelper,
) -> Result<String, OrchestratorError> {
    let mut matches = Vec::new();
    for step in &job.steps {
        let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
            continue;
        };
        if invocation.descriptor().operation() != SourceBoundOperation::RustReleasePreparedPackage {
            continue;
        }
        let Some(step_id) = step.id.as_ref() else {
            return Err(invalid("prepared_helper_authority"));
        };
        if invocation != owner.invocation() || env != owner.environment() {
            return Err(invalid("prepared_helper_authority"));
        }
        matches.push(step_id.as_str().to_owned());
    }
    let [step_id] = matches.as_slice() else {
        return Err(invalid("prepared_helper_count"));
    };
    Ok(step_id.clone())
}

fn validate_outputs(job: &ReleaseJobSpec, helper_step_id: &str) -> Result<(), OrchestratorError> {
    let expected = [
        (
            "package-artifact-id",
            "release-package-artifact",
            ActionOutput::ArtifactId,
        ),
        (
            "package-artifact-digest",
            "release-package-artifact",
            ActionOutput::ArtifactDigest,
        ),
        (
            "package-blob-sha256",
            helper_step_id,
            ActionOutput::PreparedBlobSha256,
        ),
    ];
    if job.outputs.len() != expected.len()
        || expected.iter().any(|(name, step, output)| {
            job.outputs
                .iter()
                .filter(|value| {
                    value.name == *name
                        && value.value.step_id.as_str() == *step
                        && value.value.output == *output
                })
                .count()
                != 1
        })
    {
        return Err(invalid("prepared_job_outputs"));
    }
    Ok(())
}

fn environment(policy: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "RELEASE_PACKAGE_ARTIFACT_ID".to_owned(),
            needs("package-artifact-id"),
        ),
        (
            "RELEASE_PACKAGE_ARTIFACT_DIGEST".to_owned(),
            needs("package-artifact-digest"),
        ),
        (
            "RELEASE_PACKAGE_BLOB_SHA256".to_owned(),
            needs("package-blob-sha256"),
        ),
        (
            "GITHUB_WORKSPACE".to_owned(),
            "${{ github.workspace }}".to_owned(),
        ),
        (
            "GITHUB_REPOSITORY".to_owned(),
            "${{ github.repository }}".to_owned(),
        ),
        ("GITHUB_SHA".to_owned(), "${{ github.sha }}".to_owned()),
        ("GITHUB_REF".to_owned(), "${{ github.ref }}".to_owned()),
        (
            "GITHUB_WORKFLOW_REF".to_owned(),
            "${{ github.workflow_ref }}".to_owned(),
        ),
        (
            "GITHUB_WORKFLOW_SHA".to_owned(),
            "${{ github.workflow_sha }}".to_owned(),
        ),
        (
            "GITHUB_RUN_ID".to_owned(),
            "${{ github.run_id }}".to_owned(),
        ),
        (
            "GITHUB_RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
        ("RELEASE_RECONCILE_POLICY".to_owned(), policy.to_owned()),
    ])
}

fn needs(output: &str) -> String {
    format!("${{{{ needs.{PACKAGE_JOB}.outputs.{output} }}}}")
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("release_prepared_artifact_input:{reason}"),
    }
}
