//! Closed public-repository attestation roles; compiled owners approve the graph.
use super::{
    ir::{Job, WorkflowIr},
    source_helper::HelperInvocation,
    step::StepId,
};
use crate::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Exact fixed preparation required before admission and receipt verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePublishPreparation {
    /// Unconditional preparation step identity.
    pub step_id: StepId,
    /// Compiled owner invocation.
    pub invocation: HelperInvocation,
    /// Exact owner-approved environment.
    pub environment: BTreeMap<String, String>,
}

/// Source, complete CI and immutable transfer authority for a public attestation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePublishBinding {
    /// Exact public source repository; private availability is unqualified.
    pub repository: String,
    /// Protected source branch checked by the compiled admission helper.
    pub default_branch: String,
    /// Cross-workflow complete CI proof job.
    pub full_ci_job: String,
    /// Exact compiled tag and complete CI proof invocation in that job.
    pub full_ci_admission: HelperInvocation,
    /// Immutable ZIP or index-proof artifact producer.
    pub artifact_job: String,
    /// Immutable transport artifact identifier output.
    pub artifact_id_output: String,
    /// Immutable transport artifact digest output, distinct from subject digest.
    pub artifact_digest_output: String,
    /// Exact compiled executable preparation.
    pub preparation: Vec<NativePublishPreparation>,
    /// Source/tag/CI recheck before receipt verification.
    pub admission_step: StepId,
    /// Exact compiled admission invocation.
    pub admission: HelperInvocation,
    /// Immutable transport and subject verifier step.
    pub receipt_step: StepId,
    /// Exact compiled receipt verifier invocation.
    pub receipt: HelperInvocation,
    /// Final attestation step identity.
    pub attest_step: StepId,
    /// Qualified pinned actions/attest reference from the compiled owner.
    pub attest_uses: String,
}

/// Actual immutable OCI helper identities and complete SDK execution environments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeOciPublishEnvironment {
    /// Exact configured image repository, without mutable tag or digest.
    pub image: String,
    /// Exact image identity in the OCI dependency graph.
    pub image_id: String,
    /// Closed native architecture set; the receipt uses its sorted spelling.
    pub platforms: Vec<String>,
    /// Exact SDK full CI verification environment.
    pub full_ci: BTreeMap<String, String>,
    /// Exact SDK protected source recheck environment.
    pub admission: BTreeMap<String, String>,
    /// Exact SDK immutable index receipt environment.
    pub receipt: BTreeMap<String, String>,
}

/// Supported public GitHub attestations; Apple credentials are a separate role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NativePublishRole {
    /// Push-tag signed ZIP; no GitHub Release or cask publication authority.
    DesktopTagZip {
        /// Complete exact source and immutable artifact binding.
        binding: NativePublishBinding,
        /// Literal protected attestation environment.
        environment: String,
        /// Exact single ZIP path, containing only the fixed admission version output.
        subject_path: String,
    },
    /// `DockerHub` immutable index; recovery dispatch retains all source gates.
    DockerHubIndex {
        /// Complete exact source and immutable artifact binding.
        binding: NativePublishBinding,
        /// Canonical docker.io/namespace/image subject, without tag or digest.
        subject_name: String,
        /// Exact OCI identity and compiled helper environments.
        environment: NativeOciPublishEnvironment,
        /// Closed receipt verifier's immutable index digest output name.
        subject_digest_output: String,
    },
}

impl NativePublishRole {
    /// Shared source and immutable transfer contract.
    #[must_use]
    pub fn binding(&self) -> &NativePublishBinding {
        match self {
            Self::DesktopTagZip { binding, .. } | Self::DockerHubIndex { binding, .. } => binding,
        }
    }
    /// Closed protected tag predicate; compiled admission checks exact semver/ref.
    #[must_use]
    pub fn condition(&self) -> String {
        let events = match self {
            Self::DesktopTagZip { .. } => "github.event_name == 'push'",
            Self::DockerHubIndex { .. } => {
                "(github.event_name == 'push' || github.event_name == 'workflow_dispatch')"
            }
        };
        format!(
            "success() && github.repository == '{}' && startsWith(github.ref, 'refs/tags/v') && {events}",
            self.binding().repository
        )
    }
    /// Exact source authority in the unprivileged cross-workflow CI proof job.
    #[must_use]
    pub fn full_ci_environment(&self) -> BTreeMap<String, String> {
        if let Self::DockerHubIndex { environment, .. } = self {
            return environment.full_ci.clone();
        }
        let b = self.binding();
        BTreeMap::from([
            ("APPROVED_REPOSITORY".into(), b.repository.clone()),
            ("APPROVED_DEFAULT_BRANCH".into(), b.default_branch.clone()),
            ("APPROVED_SOURCE_SHA".into(), "${{ github.sha }}".into()),
            ("APPROVED_SOURCE_REF".into(), "${{ github.ref }}".into()),
            ("GH_TOKEN".into(), "${{ github.token }}".into()),
        ])
    }
    /// Runtime authority passed only to the exact compiled source guard/verifier.
    #[must_use]
    pub fn admission_environment(&self) -> BTreeMap<String, String> {
        if let Self::DockerHubIndex { environment, .. } = self {
            return environment.admission.clone();
        }
        let b = self.binding();
        BTreeMap::from([
            ("APPROVED_REPOSITORY".into(), b.repository.clone()),
            ("APPROVED_DEFAULT_BRANCH".into(), b.default_branch.clone()),
            ("APPROVED_SOURCE_SHA".into(), "${{ github.sha }}".into()),
            ("APPROVED_SOURCE_REF".into(), "${{ github.ref }}".into()),
            (
                "FULL_CI_RESULT".into(),
                format!("${{{{ needs.{}.result }}}}", b.full_ci_job),
            ),
            (
                "EXPECTED_ARTIFACT_ID".into(),
                format!(
                    "${{{{ needs.{}.outputs.{} }}}}",
                    b.artifact_job, b.artifact_id_output
                ),
            ),
            (
                "EXPECTED_ARTIFACT_DIGEST".into(),
                format!(
                    "${{{{ needs.{}.outputs.{} }}}}",
                    b.artifact_job, b.artifact_digest_output
                ),
            ),
            ("GH_TOKEN".into(), "${{ github.token }}".into()),
        ])
    }
    /// Immutable subject identity passed to the compiled receipt verifier.
    #[must_use]
    pub fn receipt_environment(&self) -> BTreeMap<String, String> {
        if let Self::DockerHubIndex { environment, .. } = self {
            return environment.receipt.clone();
        }
        let mut environment = self.admission_environment();
        match self {
            Self::DesktopTagZip { subject_path, .. } => {
                environment.insert("SUBJECT_PATH".into(), subject_path.clone());
            }
            Self::DockerHubIndex { subject_name, .. } => {
                environment.insert("SUBJECT_NAME".into(), subject_name.clone());
            }
        }
        environment
    }
    /// Exact pinned action inputs for this immutable subject.
    #[must_use]
    pub fn attestation_inputs(&self) -> BTreeMap<String, String> {
        validation::attestation_inputs(self)
    }
    /// Validate the closed role and immutable graph before generation approval.
    /// # Errors
    /// Rejects bypasses, foreign bindings, credentials and arbitrary computation.
    pub fn validate(&self, job: &Job, workflow: &WorkflowIr) -> Result<(), ContractError> {
        validation::validate(self, job, workflow)
    }
}

pub(super) fn invalid(reason: &str) -> ContractError {
    ContractError::identity("native_publish", reason)
}
#[path = "native_publish_oci.rs"]
mod oci;
#[path = "native_publish_validation.rs"]
mod validation;

#[cfg(test)]
#[path = "native_publish_tests.rs"]
mod tests;
