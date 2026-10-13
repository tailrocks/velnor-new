//! Artifact, installed-tree, and executable observations bind exact root tool pins.
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use serde::{Deserialize, Serialize};
use velnor_actions_contract::config::{CheckPlatform, QualifiedTool, QualifiedToolArtifact};
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
