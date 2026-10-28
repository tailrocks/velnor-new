// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract::{EXPECTED_REPOSITORY, ReleaseManifest, ReleaseTarget, TargetRecord};
use velnor_actions_mise::catalog::lock::{ApprovedGeneratorRelease, ApprovedMiseRuntime};

use crate::OrchestratorError;
use crate::controller_manifest_digest::ApprovalEntry;

pub(crate) fn load_policy_approval(
    root: &Path,
    policy: velnor_actions_contract::WorkflowPolicy,
) -> Result<Option<ControllerApprovedRelease>, OrchestratorError> {
    if policy != velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(None);
    }
    if !root.join(APPROVALS_REL).is_file() {
        return Err(contract("controller_approvals:missing_approval".to_owned()));
    }
    ControllerApprovedRelease::load_active_approval(root).map(Some)
}

const APPROVALS_REL: &str = ".velnor/controller-approvals.json";
const APPROVALS_FILE: &str = "controller-approvals.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalsFile {
    schema: u32,
    repository: String,
    workflow: String,
    active: ApprovalEntry,
    staged: Option<ApprovalEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ControllerApprovedRelease {
    manifest: ReleaseManifest,
    gh_version: String,
    mise_bootstrap_version: String,
    mise_bootstrap_artifact: String,
    mise_bootstrap_sha256: String,
    mise_runtime: Vec<ApprovedMiseRuntime>,
}

impl ControllerApprovedRelease {
    pub(crate) fn load_active_approval(root: &Path) -> Result<Self, OrchestratorError> {
        let text = match crate::safe_read::read_repo_file(
            root,
            APPROVALS_REL,
            crate::safe_read::MAX_REPO_FILE_BYTES,
        )? {
            crate::safe_read::RepoRead::Text(text) => text,
            crate::safe_read::RepoRead::Absent => {
                return Err(OrchestratorError::io(
                    root.join(APPROVALS_REL).display().to_string(),
                    "controller_approvals:missing_file".to_owned(),
                ));
            }
        };
        Self::from_approvals_text(&text)
    }

    pub(crate) fn from_approvals_text(text: &str) -> Result<Self, OrchestratorError> {
        let approvals: ApprovalsFile = serde_json::from_str(text)
            .map_err(|err| contract(format!("controller_approvals:malformed_json:{err}")))?;
        approvals.validate()?;
        let active = &approvals.active;
        let manifest = ReleaseManifest {
            schema: ReleaseManifest::SCHEMA,
            version: active.version.clone(),
            repository: active.repository.clone(),
            commit: active.commit.clone(),
            targets: active
                .binaries
                .iter()
                .map(|binary| TargetRecord {
                    target: binary.target.clone(),
                    artifact: binary.artifact.clone(),
                    sha256: binary.sha256.clone(),
                })
                .collect(),
        };
        manifest
            .validate(APPROVALS_FILE)
            .map_err(|err| contract(format!("controller_approvals:manifest:{err}")))?;
        let mise_runtime = active
            .mise_runtime
            .iter()
            .map(|runtime| {
                let target = ReleaseTarget::parse_triple(&runtime.target).ok_or_else(|| {
                    contract("controller_approvals:unknown_runtime_target".to_owned())
                })?;
                Ok(ApprovedMiseRuntime {
                    target,
                    version: runtime.version.clone(),
                    sha256: runtime.sha256.clone(),
                    bytes: runtime.bytes,
                })
            })
            .collect::<Result<Vec<_>, OrchestratorError>>()?;
        Ok(Self {
            manifest,
            gh_version: active.gh.clone(),
            mise_bootstrap_version: active.mise_bootstrap.version.clone(),
            mise_bootstrap_artifact: active.mise_bootstrap.artifact.clone(),
            mise_bootstrap_sha256: active.mise_bootstrap.sha256.clone(),
            mise_runtime,
        })
    }

    pub(crate) fn as_authority(&self) -> ApprovedGeneratorRelease<'_> {
        ApprovedGeneratorRelease {
            manifest: &self.manifest,
            gh_version: &self.gh_version,
            mise_bootstrap_version: &self.mise_bootstrap_version,
            mise_bootstrap_artifact: &self.mise_bootstrap_artifact,
            mise_bootstrap_sha256: &self.mise_bootstrap_sha256,
            mise_runtime: &self.mise_runtime,
        }
    }
}

impl ApprovalsFile {
    fn validate(&self) -> Result<(), OrchestratorError> {
        if self.schema != 2 {
            return Err(contract(
                "controller_approvals:unsupported_schema".to_owned(),
            ));
        }
        if self.repository != EXPECTED_REPOSITORY {
            return Err(contract(
                "controller_approvals:unexpected_repository".to_owned(),
            ));
        }
        check_workflow(&self.workflow)?;
        if self.active.repository != self.repository {
            return Err(contract(
                "controller_approvals:entry_repository_mismatch".to_owned(),
            ));
        }
        self.active.validate("active")?;
        if let Some(staged) = &self.staged {
            if staged.repository != self.repository {
                return Err(contract(
                    "controller_approvals:entry_repository_mismatch".to_owned(),
                ));
            }
            staged.validate("staged")?;
            if !semver_greater(&staged.version, &self.active.version) {
                return Err(contract(
                    "controller_approvals:staged_must_advance".to_owned(),
                ));
            }
            if staged.commit == self.active.commit {
                return Err(contract(
                    "controller_approvals:staged_commit_reuse".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

const CONTROLLER_WORKFLOW: &str = ".github/workflows/product-release-generator.yml";

fn check_workflow(workflow: &str) -> Result<(), OrchestratorError> {
    if workflow != CONTROLLER_WORKFLOW {
        return Err(contract(
            "controller_approvals:unexpected_workflow".to_owned(),
        ));
    }
    Ok(())
}

fn semver_greater(candidate: &str, baseline: &str) -> bool {
    fn triple(version: &str) -> Option<[u64; 3]> {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let mut out = [0u64; 3];
        for (index, part) in parts.iter().enumerate() {
            out[index] = part.parse().ok()?;
        }
        Some(out)
    }
    matches!((triple(candidate), triple(baseline)), (Some(a), Some(b)) if a > b)
}

fn contract(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}

#[cfg(test)]
mod tests {
    use super::ControllerApprovedRelease;

    #[test]
    fn shipped_approvals_bind_real_manifest_digest() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let approval = ControllerApprovedRelease::from_approvals_text(&text).expect("must parse");
        assert_eq!(approval.as_authority().manifest.version, "0.1.63");
    }

    #[test]
    fn rotated_text_yields_rotated_manifest_version() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let rotated = text
            .replacen("\"version\": \"0.1.63\"", "\"version\": \"0.1.64\"", 1)
            .replace("v0.1.63", "v0.1.64")
            .replace("0.1.63-", "0.1.64-")
            .replace(
                "sha256:a868a81bba2e8dda4c43316dfd8c0c19d5aa02470eead6ada4a0de077bf92853",
                "sha256:9fa95c6a3e52e2d2fdd119ae95f14140510aa7f80bfd6d99663877160ee2067f",
            );
        let approval =
            ControllerApprovedRelease::from_approvals_text(&rotated).expect("must parse");
        assert_eq!(approval.as_authority().manifest.version, "0.1.64");
    }

    #[test]
    fn stale_manifest_digest_is_rejected() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let rotated = text
            .replacen("\"version\": \"0.1.63\"", "\"version\": \"0.1.64\"", 1)
            .replace("v0.1.63", "v0.1.64")
            .replace("0.1.63-", "0.1.64-");
        let error = ControllerApprovedRelease::from_approvals_text(&rotated)
            .expect_err("stale digest must fail");
        assert!(
            format!("{error:?}").contains("manifest_digest_mismatch"),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn non_controller_workflow_is_rejected() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let renamed = text.replace(
            ".github/workflows/product-release-generator.yml",
            ".github/workflows/ci.yml",
        );
        let error = ControllerApprovedRelease::from_approvals_text(&renamed)
            .expect_err("foreign workflow must fail");
        assert!(
            format!("{error:?}").contains("unexpected_workflow"),
            "unexpected error: {error:?}"
        );
    }
}
