//! Artifact, installed-tree, and executable observations bind exact root tool pins.
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use serde::{Deserialize, Serialize};
use velnor_actions_contract::config::{
    CheckPlatform, QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend,
    QualifiedToolOptions, QualifiedToolProbe,
};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_mise::check_tool_probes::QualifiedExecutableProof;

/// Actual installation identity carried inside the ordinary check execution receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QualifiedToolReceipt {
    pub id: String,
    pub version: String,
    pub platform: CheckPlatform,
    pub definition_digest: String,
    pub artifacts: Vec<QualifiedToolArtifact>,
    pub dependency_artifacts: Vec<QualifiedToolArtifact>,
    pub install_tree_sha256: String,
    pub executables: Vec<QualifiedExecutableProof>,
}

/// Untrusted observations collected for a candidate pin; never accepted as a receipt.
///
/// Probe output is external evidence. A caller must review and explicitly promote
/// these values into repository configuration before ordinary acquisition can use
/// them as expected pins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UntrustedToolMeasurement {
    /// Versioned candidate record format.
    pub schema: String,
    /// Candidate tool identity.
    pub tool_id: String,
    /// Declared upstream backend.
    pub backend: QualifiedToolBackend,
    /// Exact declared tool release.
    pub version: String,
    /// Declared backend-specific install options.
    pub options: QualifiedToolOptions,
    /// Declared direct prerequisite identities; their bytes are not measured here.
    pub depends_on: Vec<String>,
    /// Native platform on which the candidate was measured.
    pub platform: CheckPlatform,
    /// Declared source URLs and expected archive hashes whose bytes were verified.
    pub source_artifacts: Vec<QualifiedToolArtifact>,
    /// Source archives for dependencies used by this candidate; empty for supported inputs.
    pub dependency_source_artifacts: Vec<QualifiedToolArtifact>,
    /// SHA-256 observed for the complete normalized installation tree.
    pub measured_install_tree_sha256: String,
    /// Observed executable hashes and fixed-probe results.
    pub executable_observations: Vec<MeasuredToolExecutable>,
}

/// One candidate executable identity and its bounded version-probe observation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MeasuredToolExecutable {
    /// Stable executable identity.
    pub name: String,
    /// Relative executable path below the normalized prefix.
    pub path: String,
    /// SHA-256 measured from the normalized executable bytes.
    pub measured_sha256: String,
    /// Fixed version probe declaration.
    pub probe: QualifiedToolProbe,
    /// Bounded stdout observed from executing the candidate probe.
    pub stdout: String,
    /// BLAKE3 digest of the exact observed stdout bytes.
    pub stdout_digest: String,
    /// BLAKE3 digest of the exact observed stderr bytes.
    pub stderr_digest: String,
}

pub(crate) fn receipt(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    artifacts: Vec<QualifiedToolArtifact>,
    dependency_artifacts: Vec<QualifiedToolArtifact>,
    install_tree_sha256: String,
    executables: Vec<QualifiedExecutableProof>,
) -> Result<QualifiedToolReceipt, OrchestratorError> {
    let receipt = QualifiedToolReceipt {
        id: tool.id.clone(),
        version: tool.version.clone(),
        platform,
        definition_digest: digest_b3(&canonical_json_bytes(tool).map_err(internal_contract)?),
        artifacts,
        dependency_artifacts,
        install_tree_sha256,
        executables,
    };
    if !valid_receipt(platform, tool, &receipt) {
        return Err(internal("qualified_tool_proof_identity"));
    }
    Ok(receipt)
}

/// Required compares observations against the complete resolved dependency closure.
pub(crate) fn validate_receipts(
    platform: CheckPlatform,
    tools: &[QualifiedTool],
    receipts: &[QualifiedToolReceipt],
) -> bool {
    tools.len() == receipts.len()
        && tools
            .iter()
            .zip(receipts)
            .all(|(tool, proof)| valid_receipt(platform, tool, proof))
}

fn valid_receipt(
    platform: CheckPlatform,
    tool: &QualifiedTool,
    receipt: &QualifiedToolReceipt,
) -> bool {
    let Some(qualified) = tool.platforms.iter().find(|p| p.platform == platform) else {
        return false;
    };
    let Ok(definition) = canonical_json_bytes(tool) else {
        return false;
    };
    receipt.id == tool.id
        && receipt.version == tool.version
        && receipt.platform == platform
        && receipt.definition_digest == digest_b3(&definition)
        && receipt.artifacts == qualified.artifacts
        && receipt.dependency_artifacts == qualified.dependency_artifacts
        && receipt.install_tree_sha256 == qualified.install_tree_sha256
        && receipt.executables.iter().all(|proof| {
            let suffix = std::path::Path::new("tools")
                .join(&tool.id)
                .join("prefix")
                .join(&proof.declared.path);
            proof.observed.path.ends_with(suffix)
                && proof.observed.path.components().all(|component| {
                    matches!(
                        component,
                        std::path::Component::RootDir | std::path::Component::Normal(_)
                    )
                })
        })
        && velnor_actions_mise::check_tool_probes::validate_executable_proofs(
            tool,
            platform,
            &receipt.executables,
        )
        .is_ok()
}

#[cfg(test)]
#[path = "check_tool_proof_tests.rs"]
mod tests;
