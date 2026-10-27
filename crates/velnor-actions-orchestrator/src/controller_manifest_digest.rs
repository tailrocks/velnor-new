// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_actions_contract::ids::is_lower_hex_len;

use crate::OrchestratorError;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalEntry {
    pub(crate) version: String,
    pub(crate) commit: String,
    pub(crate) repository: String,
    pub(crate) manifest_digest: String,
    pub(crate) binaries: Vec<ApprovalBinary>,
    pub(crate) gh: String,
    pub(crate) mise: ApprovalMise,
    pub(crate) state: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalBinary {
    pub(crate) target: String,
    pub(crate) artifact: String,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalMise {
    pub(crate) version: String,
    pub(crate) artifact: String,
    pub(crate) sha256: String,
}

const KNOWN_TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

impl ApprovalEntry {
    pub(crate) fn validate(&self, slot: &str) -> Result<(), OrchestratorError> {
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
        let mut expected = String::from("sha256:");
        expected.push_str(&canonical_manifest_digest(self));
        if expected != self.manifest_digest {
            return Err(contract(
                "controller_approvals:manifest_digest_mismatch".to_owned(),
            ));
        }
        Ok(())
    }
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

fn is_https_url(url: &str) -> bool {
    url.starts_with("https://")
        && !url.bytes().any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn contract(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}
