//! Closed job output references to declared action and native helper steps.
use super::source_helper::SourceBoundOperation::{
    MbxProducerReport, NativePublishReceiptVerifier, OciDelivery, SourceProducerReport,
    ToolProducerReport,
};
use super::step::{Step, StepId, StepKind};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One exported job output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobOutput {
    /// Literal output identifier.
    pub name: String,
    /// Closed reference to a declared step output.
    pub value: StepOutputRef,
}

/// A declared step's known action output; never an arbitrary expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepOutputRef {
    /// Exact declared action step identifier.
    pub step_id: StepId,
    /// Action-defined output name.
    pub output: ActionOutput,
}

/// Supported outputs from closed actions and compiled native helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionOutput {
    /// Immutable artifact identifier from upload-artifact.
    ArtifactId,
    /// Artifact digest from upload-artifact.
    ArtifactDigest,
    /// Deployed Pages URL from deploy-pages.
    PageUrl,
    /// Terminal cache transport availability.
    CacheAvailable,
    /// Terminal exact verification status.
    Verified,
    /// Source cache identity from the closed source report.
    SourceIdentity,
    /// SHA-256 of the original canonical source snapshot ZIP.
    SourceSnapshotBlobSha256,
    /// Exact source commit proven by the source API snapshot.
    SourceCommitSha,
    /// Exact root source tree proven by the source API snapshot.
    SourceTreeSha,
    /// SHA-256 of the original prepared package ZIP from its closed producer.
    PreparedBlobSha256,
    /// Executable cache identity from the closed tool report.
    ToolIdentity,
    /// Stable compiled executable descriptor identity.
    DescriptorIdentity,
    /// Authenticated native OCI index subject digest, distinct from archive digests.
    OciIndexDigest,
    /// Verified literal release version from the closed OCI source verifier.
    ReleaseVersion,
    /// Authenticated immutable OCI recovery existence decision.
    OciExisting,
    /// Terminal typed producer error.
    Error,
}

impl ActionOutput {
    /// Native action output spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArtifactId => "artifact-id",
            Self::ArtifactDigest => "artifact-digest",
            Self::PageUrl => "page_url",
            Self::CacheAvailable => "cache_available",
            Self::Verified => "verified",
            Self::SourceIdentity => "sourceidentity",
            Self::SourceSnapshotBlobSha256 => "source-snapshot-blob-sha256",
            Self::SourceCommitSha => "source-commit-sha",
            Self::SourceTreeSha => "source-tree-sha",
            Self::PreparedBlobSha256 => "package-blob-sha256",
            Self::ToolIdentity => "toolidentity",
            Self::DescriptorIdentity => "descriptoridentity",
            Self::OciIndexDigest => "index_digest",
            Self::ReleaseVersion => "version",
            Self::OciExisting => "existing",
            Self::Error => "error",
        }
    }

    fn action(self) -> Option<&'static str> {
        match self {
            Self::ArtifactId | Self::ArtifactDigest => Some("actions/upload-artifact"),
            Self::PageUrl => Some("actions/deploy-pages"),
            _ => None,
        }
    }

    fn helper(self, invocation: &super::source_helper::HelperInvocation) -> bool {
        let operation = invocation.descriptor().operation();
        match self {
            Self::CacheAvailable | Self::Verified | Self::Error => {
                matches!(
                    operation,
                    SourceProducerReport | ToolProducerReport | MbxProducerReport
                )
            }
            Self::SourceIdentity => matches!(operation, SourceProducerReport | MbxProducerReport),
            Self::SourceSnapshotBlobSha256 | Self::SourceCommitSha | Self::SourceTreeSha => {
                operation == super::source_helper::SourceBoundOperation::RustReleaseSourceSnapshot
                    && invocation.args().is_empty()
            }
            Self::PreparedBlobSha256 => {
                operation == super::source_helper::SourceBoundOperation::RustReleasePreparedPackage
                    && invocation.args().is_empty()
            }
            Self::ToolIdentity | Self::DescriptorIdentity => operation == ToolProducerReport,
            Self::ReleaseVersion | Self::OciExisting => {
                let phase = if self == Self::ReleaseVersion {
                    "verify"
                } else {
                    "admission"
                };
                operation == super::source_helper::SourceBoundOperation::OciDelivery
                    && invocation.args().len() == 4
                    && invocation
                        .args()
                        .first()
                        .is_some_and(|value| value == phase)
            }
            Self::OciIndexDigest => {
                operation == NativePublishReceiptVerifier
                    || operation == OciDelivery
                        && invocation.args().len() == 4
                        && invocation.args().first().is_some_and(|phase| {
                            matches!(phase.as_str(), "admission" | "assembly" | "index-receipt")
                        })
            }
            _ => false,
        }
    }
}

impl StepOutputRef {
    /// Render this closed reference as a GitHub Actions expression.
    #[must_use]
    pub fn expression(&self) -> String {
        format!(
            "${{{{ steps.{}.outputs.{} }}}}",
            self.step_id.as_str(),
            self.output.as_str()
        )
    }

    fn validate(&self, steps: &[Step]) -> Result<(), ContractError> {
        self.step_id.validate()?;
        let step = steps
            .iter()
            .find(|step| step.id.as_ref() == Some(&self.step_id));
        let valid = step.is_some_and(|step| match &step.kind {
            StepKind::Action { uses, .. } => uses.split_once('@').is_some_and(|(action, sha)| {
                Some(action) == self.output.action()
                    && sha.len() == 40
                    && sha.bytes().all(|b| b.is_ascii_hexdigit())
            }),
            StepKind::SourceBoundHelper { invocation, .. } => self.output.helper(invocation),
            _ => false,
        });
        if !valid {
            return Err(ContractError::identity(
                "job.outputs",
                "unknown_step_output",
            ));
        }
        Ok(())
    }
}

/// Validate output names, uniqueness, and exact action step references.
/// # Errors
/// Rejects malformed, duplicated, or undeclared output bindings.
pub fn validate_job_outputs(outputs: &[JobOutput], steps: &[Step]) -> Result<(), ContractError> {
    super::step::validate_step_ids(steps)?;
    let mut names = BTreeSet::new();
    for output in outputs {
        StepId::new(&output.name)?;
        if !names.insert(output.name.as_str()) {
            return Err(ContractError::identity("job.outputs", "duplicate_output"));
        }
        output.value.validate(steps)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "outputs_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "outputs_snapshot_tests.rs"]
mod snapshot_tests;
