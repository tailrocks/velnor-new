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
    mise_version: String,
    mise_artifact: String,
    mise_sha256: String,
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
            mise_version: active.mise.version.clone(),
            mise_artifact: active.mise.artifact.clone(),
            mise_sha256: active.mise.sha256.clone(),
        })
    }

    pub(crate) fn as_authority(&self) -> ApprovedGeneratorRelease<'_> {
        ApprovedGeneratorRelease {
            manifest: &self.manifest,
            gh_version: &self.gh_version,
            mise_version: &self.mise_version,
            mise_artifact: &self.mise_artifact,
            mise_sha256: &self.mise_sha256,
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
        for binary in &self.binaries {
            if !KNOWN_TARGETS.contains(&binary.target.as_str()) {
                return Err(contract(
                    "controller_approvals:unknown_target".to_owned(),
                ));
            }
            if !is_https_url(&binary.artifact) {
                return Err(contract(
                    "controller_approvals:malformed_url".to_owned(),
                ));
            }
        }
        let expected = format!("sha256:{}", canonical_manifest_digest(self));
        if expected != self.manifest_digest {
            return Err(contract(
                "controller_approvals:manifest_digest_mismatch".to_owned(),
            ));
        }
        Ok(())
    }
}

const CONTROLLER_WORKFLOW: &str = ".github/workflows/product-release-generator.yml";

const KNOWN_TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

fn check_workflow(workflow: &str) -> Result<(), OrchestratorError> {
    if workflow != CONTROLLER_WORKFLOW {
        return Err(contract(
            "controller_approvals:unexpected_workflow".to_owned(),
        ));
    }
    Ok(())
}

fn canonical_manifest_digest(entry: &ApprovalEntry) -> String {
    use sha2::{Digest, Sha256};
    let mut json = String::from("{\n");
    json.push_str("  \"schema\": 1,\n");
    push_field(&mut json, 2, "version", &entry.version, true);
    push_field(&mut json, 2, "repository", &entry.repository, true);
    push_field(&mut json, 2, "commit", &entry.commit, true);
    json.push_str("  \"targets\": [\n");
    for (index, binary) in entry.binaries.iter().enumerate() {
        json.push_str("    {\n");
        push_field(&mut json, 6, "target", &binary.target, true);
        push_field(&mut json, 6, "artifact", &binary.artifact, true);
        push_field(&mut json, 6, "sha256", &binary.sha256, false);
        json.push_str("    }");
        if index + 1 < entry.binaries.len() {
            json.push(',');
        }
        json.push('\n');
    }
    json.push_str("  ]\n}\n");
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(json.as_bytes()) {
        digest.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        digest.push(HEX_DIGITS[(byte & 0x0F) as usize] as char);
    }
    digest
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn push_field(json: &mut String, indent: usize, key: &str, value: &str, comma: bool) {
    for _ in 0..indent {
        json.push(' ');
    }
    json.push('"');
    json.push_str(key);
    json.push_str("\": \"");
    push_escaped(json, value);
    json.push('"');
    if comma {
        json.push(',');
    }
    json.push('\n');
}

fn push_escaped(json: &mut String, value: &str) {
    for point in value.chars() {
        match point {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\u{08}' => json.push_str("\\b"),
            '\u{09}' => json.push_str("\\t"),
            '\u{0A}' => json.push_str("\\n"),
            '\u{0C}' => json.push_str("\\f"),
            '\u{0D}' => json.push_str("\\r"),
            '\u{00}'..='\u{1F}' | '\u{7F}' => {
                let code = point as u32;
                json.push_str("\\u00");
                json.push(HEX_DIGITS[(code >> 4) as usize] as char);
                json.push(HEX_DIGITS[(code & 0x0F) as usize] as char);
            }
            _ => json.push(point),
        }
    }
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
    fn shipped_approvals_bind_real_manifest_digest() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let approval =
            ControllerApprovedRelease::from_approvals_text(&text).expect("must parse");
        assert_eq!(approval.as_authority().manifest.version, "0.1.55");
    }

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
            .replace("0.1.55-", "0.1.60-")
            .replace(
                "sha256:90ee052ab599c9c064afce1d26b65e6752c4b7cd8feecd3c57249ea0241afbd3",
                "sha256:0fcf85c8d54d658154b0466fcd02a505ec79dc0c1ba07f2c5f3193b2de4b2295",
            );
        let approval =
            ControllerApprovedRelease::from_approvals_text(&rotated).expect("must parse");
        assert_eq!(approval.as_authority().manifest.version, "0.1.60");
    }

    #[test]
    fn stale_manifest_digest_is_rejected() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let rotated = text
            .replacen("\"version\": \"0.1.55\"", "\"version\": \"0.1.60\"", 1)
            .replace("v0.1.55", "v0.1.60")
            .replace("0.1.55-", "0.1.60-");
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
