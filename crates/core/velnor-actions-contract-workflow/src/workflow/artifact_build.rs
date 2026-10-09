//! Plan-bound identities and exact reconciliation for declared build outputs.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::{ArtifactBuildOutput, ArtifactBuildTask};

/// Plan-step output containing the GitHub-hosted artifact task matrix.
pub const ARTIFACT_HOSTED_MATRIX_OUTPUT: &str = "artifact_hosted_matrix";
/// Plan-step output containing the Velnor Scale Set artifact task matrix.
pub const ARTIFACT_VELNOR_MATRIX_OUTPUT: &str = "artifact_velnor_matrix";
/// Matrix marker: producer job carrying the provider-specific output.
pub const ARTIFACT_MATRIX_NEEDS_JOB_ENV: &str = "VELNOR_ARTIFACT_MATRIX_NEEDS_JOB";
/// Matrix marker: provider output selector (`provider` is the only accepted value).
pub const ARTIFACT_MATRIX_PROVIDER_ENV: &str = "VELNOR_ARTIFACT_MATRIX_PROVIDER";
/// Matrix marker: per-provider `strategy.max-parallel` limit.
pub const ARTIFACT_MATRIX_MAX_PARALLEL_ENV: &str = "VELNOR_ARTIFACT_MATRIX_MAX_PARALLEL";
/// Internal operation that verifies and materializes one declared task output.
pub const ARTIFACT_EXPORT_OPERATION: &str = "export-artifact-v1";
/// Internal operation that exports outputs from an existing verification job.
pub const VERIFICATION_ARTIFACT_EXPORT_OPERATION: &str = "export-verification-artifact-v1";
/// Matrix task ID consumed by the artifact exporter.
pub const ARTIFACT_TASK_ID_ENV: &str = "VELNOR_ARTIFACT_TASK_ID";
/// Matrix Mise task name consumed by the artifact exporter.
pub const ARTIFACT_MISE_TASK_ENV: &str = "VELNOR_ARTIFACT_MISE_TASK";
/// Matrix provider token consumed by the artifact exporter.
pub const ARTIFACT_PROVIDER_ENV: &str = "VELNOR_ARTIFACT_PROVIDER";
/// Matrix source commit consumed by the artifact exporter.
pub const ARTIFACT_SOURCE_SHA_ENV: &str = "VELNOR_ARTIFACT_SOURCE_SHA";
/// Matrix plan digest consumed by the artifact exporter.
pub const ARTIFACT_PLAN_DIGEST_ENV: &str = "VELNOR_ARTIFACT_PLAN_DIGEST";
/// Exact run-scoped artifact name carried by the authoritative plan matrix.
pub const ARTIFACT_NAME_ENV: &str = "VELNOR_ARTIFACT_NAME";
/// Bounded receipt linking Actions API jobs and downloaded build outputs.
pub const ARTIFACT_BUILD_OBSERVATIONS_FILENAME: &str = "artifact-build-observations.json";
/// Result-manifest basename inside one provider/task artifact.
pub const ARTIFACT_BUILD_RESULT_FILENAME: &str = "artifact-result.json";
/// Directory containing declared outputs inside one provider/task artifact.
pub const ARTIFACT_BUILD_OUTPUTS_DIRECTORY: &str = "outputs";

mod expected;
mod identity;
mod matrix;
mod observation;
mod reconcile;
mod result;

pub use expected::{artifact_plan_providers, expected_artifact_builds};
pub use identity::{
    artifact_name, artifact_name_for, artifact_name_for_run_key, canonical_plan_digest,
};
pub use matrix::{ArtifactBuildMatrix, ArtifactBuildMatrixEntry, artifact_matrix_for_provider};
pub use observation::{ArtifactBuildObservation, DownloadedArtifactOutput};
pub use reconcile::reconcile_artifact_builds;
pub use result::export_artifact_result;

/// One provider executing one declared artifact task in a workflow run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildIdentity {
    /// Numeric GitHub repository ID, preserved as canonical decimal text.
    pub repository_id: String,
    /// Exact `owner/repository` slug supplied by GitHub.
    pub repository: String,
    /// Full 40-character source commit checked out by the job.
    pub source_sha: String,
    /// BLAKE3 digest of the canonical authoritative plan bytes.
    pub plan_digest: String,
    /// GitHub workflow run ID, represented without arithmetic conversion.
    pub run_id: String,
    /// GitHub workflow run attempt (starts at one).
    pub run_attempt: u32,
    /// GitHub workflow job key (`GITHUB_JOB`), not the numeric API job ID.
    pub workflow_job_id: String,
    /// Actual provider running this task.
    pub provider: ArtifactBuildProvider,
    /// Exact logical task ID from the plan.
    pub task_id: String,
}

/// GitHub-owned run context used to derive every expected artifact identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildRunContext {
    /// Numeric GitHub repository ID, preserved as canonical decimal text.
    pub repository_id: String,
    /// Exact `owner/repository` slug supplied by GitHub.
    pub repository: String,
    /// GitHub workflow run ID, represented without arithmetic conversion.
    pub run_id: String,
    /// GitHub workflow run attempt (starts at one).
    pub run_attempt: u32,
}

/// Provider identity for an artifact-producing build task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactBuildProvider {
    /// Genuine GitHub-hosted runner.
    GithubHosted,
    /// Velnor-controlled official Scale Set runner container.
    VelnorScaleSet,
}

/// One declared artifact task after its provider placement is fixed by the
/// selected workflow mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildTaskPlan {
    /// Validated task definition from repository configuration.
    pub task: ArtifactBuildTask,
    /// Fixed producer path. The default matrix path is omitted for old plans.
    #[serde(
        default,
        skip_serializing_if = "ArtifactBuildProducer::is_matrix_build"
    )]
    pub producer: ArtifactBuildProducer,
    /// Exact providers selected for this run, in stable order.
    pub providers: Vec<ArtifactBuildProvider>,
}

/// One output producer kind represented in the shared artifact receipt plan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactBuildProducer {
    /// The existing plan-driven artifact-build matrix job.
    #[default]
    MatrixBuild,
    /// A static verification job that already executes this task once.
    VerificationTask,
}

impl ArtifactBuildProducer {
    /// Whether this is the legacy matrix producer with its old wire shape.
    #[must_use]
    pub const fn is_matrix_build(&self) -> bool {
        matches!(self, Self::MatrixBuild)
    }
}

impl ArtifactBuildTaskPlan {
    /// Validate task identity and its non-empty, sorted provider set.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        self.task.validate("plan")?;
        if self.providers.is_empty() {
            return Err(ContractError::identity(
                "artifact.providers",
                "empty_provider_inventory",
            ));
        }
        let mut previous = None;
        for provider in &self.providers {
            if previous.is_some_and(|value| value >= *provider) {
                return Err(ContractError::identity(
                    "artifact.providers",
                    "providers_must_be_sorted_unique",
                ));
            }
            previous = Some(*provider);
        }
        Ok(())
    }
}

impl ArtifactBuildProvider {
    /// Stable provider token used in GitHub artifact names.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::GithubHosted => "hosted",
            Self::VelnorScaleSet => "velnor",
        }
    }

    /// Stable provider value serialized into the task matrix.
    #[must_use]
    pub const fn matrix_token(self) -> &'static str {
        match self {
            Self::GithubHosted => "github_hosted",
            Self::VelnorScaleSet => "velnor_scale_set",
        }
    }

    /// Workflow job key for a provider-specific artifact matrix.
    #[must_use]
    pub const fn workflow_job_id(self, paired: bool) -> &'static str {
        match (self, paired) {
            (Self::GithubHosted | Self::VelnorScaleSet, false) => "artifact-build",
            (Self::GithubHosted, true) => "artifact-build__hosted",
            (Self::VelnorScaleSet, true) => "artifact-build__local",
        }
    }

    /// Explicit matrix-expanded GitHub job display name for API correlation.
    #[must_use]
    pub fn workflow_job_name(self, task_id: &str) -> String {
        format!("Build artifact {} / {task_id}", self.matrix_token())
    }

    /// Plan-step output carrying this provider's exact task matrix.
    #[must_use]
    pub const fn matrix_output(self) -> &'static str {
        match self {
            Self::GithubHosted => ARTIFACT_HOSTED_MATRIX_OUTPUT,
            Self::VelnorScaleSet => ARTIFACT_VELNOR_MATRIX_OUTPUT,
        }
    }
}

/// One nonempty, bounded file captured by an artifact build job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildFile {
    /// Declared output ID.
    pub output_id: String,
    /// Exact repository-relative path from the plan.
    pub path: String,
    /// Raw file length before upload.
    pub size_bytes: u64,
    /// BLAKE3 digest of the raw file bytes.
    pub digest: String,
}

/// Machine-readable result uploaded with one provider/task artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildResult {
    /// Result schema version; must be one.
    pub schema: u32,
    /// Full provider- and attempt-scoped GitHub artifact name.
    pub artifact_name: String,
    /// Immutable run, source, plan, provider, and task identity.
    pub identity: ArtifactBuildIdentity,
    /// Exact declared nonempty output inventory and checksums.
    pub outputs: Vec<ArtifactBuildFile>,
}

/// One expected provider/task result derived from the validated plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildExpectation {
    /// Required identity for this provider/task pair.
    pub identity: ArtifactBuildIdentity,
    /// Exact workflow display name for static producers; omitted for legacy
    /// matrix producers and derived from identity when reading older plans.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workflow_job_name: String,
    /// Producer path that determines how the job was emitted.
    #[serde(
        default,
        skip_serializing_if = "ArtifactBuildProducer::is_matrix_build"
    )]
    pub producer: ArtifactBuildProducer,
    /// Exact output declarations from the authoritative plan.
    pub outputs: Vec<ArtifactBuildOutput>,
}

impl ArtifactBuildExpectation {
    /// Exact Actions API job name, including static-lane display suffixes.
    #[must_use]
    pub fn expected_workflow_job_name(&self) -> String {
        if self.workflow_job_name.is_empty() {
            self.identity
                .provider
                .workflow_job_name(&self.identity.task_id)
        } else {
            self.workflow_job_name.clone()
        }
    }
}

pub(super) fn validate_repository_identity(
    repository_id: &str,
    repository: &str,
) -> Result<(), ContractError> {
    if repository_id
        .parse::<u64>()
        .ok()
        .is_none_or(|id| id == 0 || id.to_string() != repository_id)
    {
        return Err(ContractError::identity(
            "artifact.repository_id",
            "bad_repository_id",
        ));
    }
    if !valid_repository_slug(repository) {
        return Err(ContractError::identity(
            "artifact.repository",
            "bad_repository_slug",
        ));
    }
    Ok(())
}

fn valid_repository_slug(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !repository.is_empty()
        && !repository.contains('/')
        && value.len() <= 201
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

#[cfg(test)]
mod tests;
