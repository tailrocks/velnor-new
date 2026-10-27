// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract::{
    EXPECTED_REPOSITORY, ReleaseManifest, TargetRecord, ids::is_lower_hex_len,
};
use velnor_actions_mise::catalog::lock::ApprovedGeneratorRelease;

use crate::OrchestratorError;

pub(crate) fn load_policy_approval(
    root: &Path,
    policy: velnor_actions_contract::WorkflowPolicy,
) -> Result<Option<ControllerApprovedRelease>, OrchestratorError> {
    if policy != velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(None);
    }
    if !root.join(APPROVALS_REL).is_file() {
        return Err(contract(
            "controller_approvals:missing_approval".to_owned(),
        ));
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalEntry {
    version: String,
    commit: String,
    repository: String,
    manifest_digest: String,
    binaries: Vec<ApprovalBinary>,
    gh: String,
    mise: ApprovalMise,
    state: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalBinary {
    target: String,
    artifact: String,
    sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalMise {
    version: String,
    artifact: String,
    sha256: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ControllerApprovedRelease {
    manifest: ReleaseManifest,
    gh_version: String,
}

impl ControllerApprovedRelease {
    pub(crate) fn load_active_approval(root: &Path) -> Result<Self, OrchestratorError> {
        let path = root.join(APPROVALS_REL);
        let text = std::fs::read_to_string(&path).map_err(|err| {
            OrchestratorError::io(path.display().to_string(), err.to_string())
        })?;
        Self::from_approvals_text(&text)
    }

    pub(crate) fn from_approvals_text(text: &str) -> Result<Self, OrchestratorError> {
        let approvals: ApprovalsFile = serde_json::from_str(text).map_err(|err| {
            contract(format!("controller_approvals:malformed_json:{err}"))
        })?;
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
        Ok(Self {
            manifest,
            gh_version: active.gh.clone(),
        })
    }

    pub(crate) fn as_authority(&self) -> ApprovedGeneratorRelease<'_> {
        ApprovedGeneratorRelease {
            manifest: &self.manifest,
            gh_version: &self.gh_version,
        }
    }
}

impl ApprovalsFile {
    fn validate(&self) -> Result<(), OrchestratorError> {
        if self.schema != 1 {
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

impl ApprovalEntry {
    fn validate(&self, slot: &str) -> Result<(), OrchestratorError> {
        if self.state != slot {
            return Err(contract(format!(
                "controller_approvals:state_slot_mismatch:{slot}"
            )));
        }
        check_semver_format(&self.version).map_err(|()| {
            contract("controller_approvals:malformed_semver".to_owned())
        })?;
        if !is_lower_hex_len(&self.commit, 40) {
            return Err(contract(
                "controller_approvals:malformed_commit".to_owned(),
            ));
        }
        let digest = self.manifest_digest.strip_prefix("sha256:").ok_or_else(|| {
            contract("controller_approvals:malformed_sha256".to_owned())
        })?;
        if !is_lower_hex_len(digest, 64) {
            return Err(contract(
                "controller_approvals:malformed_sha256".to_owned(),
            ));
        }
        if self.binaries.len() != 3 {
            return Err(contract(
                "controller_approvals:missing_target".to_owned(),
            ));
        }
        for binary in &self.binaries {
            if !is_lower_hex_len(&binary.sha256, 64) {
                return Err(contract(
                    "controller_approvals:malformed_sha256".to_owned(),
                ));
            }
        }
        check_semver_format(&self.gh)
            .map_err(|()| contract("controller_approvals:malformed_semver".to_owned()))?;
        check_semver_format(&self.mise.version)
            .map_err(|()| contract("controller_approvals:malformed_semver".to_owned()))?;
        if !is_lower_hex_len(&self.mise.sha256, 64) {
            return Err(contract(
                "controller_approvals:malformed_sha256".to_owned(),
            ));
        }
        if !is_https_url(&self.mise.artifact) {
            return Err(contract(
                "controller_approvals:malformed_url".to_owned(),
            ));
        }
        Ok(())
    }
}

fn check_workflow(workflow: &str) -> Result<(), OrchestratorError> {
    if !workflow.starts_with(".github/workflows/")
        || !std::path::Path::new(workflow)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml"))
    {
        return Err(contract(
            "controller_approvals:unexpected_workflow".to_owned(),
        ));
    }
    if workflow.bytes().any(|byte| {
        byte.is_ascii_whitespace()
            || byte.is_ascii_control()
            || matches!(byte, b'$' | b'`' | b'?' | b'#')
    }) {
        return Err(contract(
            "controller_approvals:unexpected_workflow".to_owned(),
        ));
    }
    Ok(())
}

fn check_semver_format(version: &str) -> Result<(), ()> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(());
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

fn is_https_url(url: &str) -> bool {
    url.starts_with("https://")
        && !url.bytes().any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn contract(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}

#[cfg(test)]
mod tests {
    use super::ControllerApprovedRelease;

    #[test]
    fn rotated_text_yields_rotated_manifest_version() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let rotated = text
            .replacen("\"version\": \"0.1.55\"", "\"version\": \"0.1.60\"", 1)
            .replace("v0.1.55", "v0.1.60")
            .replace("0.1.55-", "0.1.60-");
        let approval =
            ControllerApprovedRelease::from_approvals_text(&rotated).expect("must parse");
        assert_eq!(approval.as_authority().manifest.version, "0.1.60");
    }
}
