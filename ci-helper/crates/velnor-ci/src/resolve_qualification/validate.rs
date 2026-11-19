// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The checks the legacy contract crate makes before it accepts a dispatch context, a receipt
//! lineage or a source delta. Every problem keeps the legacy text, `invalid <field>: <problem>`
//! for a contract identity error.

use serde::Serialize;

use super::types::{
    Admission, CI_WORKFLOW_PATH, Dispatch, MAX_DEPTH, MAX_LANES, MAX_RECEIPT_BYTES, Node, Phase,
    RECEIPT_ARTIFACT, Receipt, RunRef, SourceDelta,
};
use crate::error::CiError;
use crate::write_request::{canonical, internal};

const MAX_NESTING: usize = 32;
const MAX_TEXT_BYTES: usize = 4_096;

pub(super) fn identity(field: &str, problem: &str) -> CiError {
    internal(&format!("invalid {field}: {problem}"))
}

fn invalid(problem: &str) -> CiError {
    identity("qualification.cache_receipt", problem)
}

pub(super) fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CiError> {
    serde_json::to_value(value)
        .map(|json| canonical::bytes(&json))
        .map_err(|error| internal(&format!("canonical json error: {error}")))
}

pub(super) fn digest_b3(bytes: &[u8]) -> String {
    format!("b3-{}", blake3::hash(bytes).to_hex())
}

fn validate_digest(value: &str) -> Result<(), CiError> {
    let valid = value.strip_prefix("b3-").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    });
    if valid {
        Ok(())
    } else {
        Err(identity("digest", "malformed_b3_digest"))
    }
}

fn normalize_posix_path(path: &str) -> Result<String, CiError> {
    if path.is_empty() {
        return Err(identity("path", "empty_path"));
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(identity("path", "absolute_path"));
    }
    if normalized.split('/').any(|segment| segment == "..") {
        return Err(identity("path", "parent_traversal"));
    }
    Ok(normalized)
}

impl RunRef {
    pub(super) fn validate(self) -> Result<(), CiError> {
        if self.run_id == 0 || self.run_attempt == 0 {
            return Err(identity(
                "qualification.predecessor",
                "invalid_run_reference",
            ));
        }
        Ok(())
    }
}

fn is_ci_run_key(value: &str) -> bool {
    let Some((run, attempt)) = value
        .strip_prefix('r')
        .and_then(|rest| rest.split_once("-a"))
    else {
        return false;
    };
    !run.is_empty()
        && !attempt.is_empty()
        && run.bytes().all(|byte| byte.is_ascii_digit())
        && attempt.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_plan_id(value: &str) -> Result<(), CiError> {
    let valid = value
        .strip_prefix("plan-")
        .is_some_and(|key| key == "local" || is_ci_run_key(key));
    if valid {
        Ok(())
    } else {
        Err(identity("plan_id", "malformed_plan_id"))
    }
}

fn lower_hex_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_branch_name(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.starts_with('.')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

impl Dispatch {
    pub(super) fn validate_for(
        &self,
        default_branch: &str,
        expected_repository: &str,
        expected_source_sha: &str,
    ) -> Result<(), CiError> {
        self.validate_shape()?;
        if self.repository != expected_repository {
            return Err(identity("qualification.repository", "repository_mismatch"));
        }
        if self.default_branch != default_branch
            || self.git_ref != format!("refs/heads/{default_branch}")
        {
            return Err(identity("qualification.ref", "not_default_branch"));
        }
        if !self.ref_protected {
            return Err(identity("qualification.ref_protected", "unprotected_ref"));
        }
        let workflow_ref = format!(
            "{}/{CI_WORKFLOW_PATH}@refs/heads/{default_branch}",
            self.repository
        );
        if self.workflow_ref != workflow_ref {
            return Err(identity(
                "qualification.workflow_ref",
                "workflow_ref_mismatch",
            ));
        }
        if self.source_sha != expected_source_sha
            || self.workflow_sha != expected_source_sha
            || self.source_sha != self.workflow_sha
        {
            return Err(identity(
                "qualification.source_sha",
                "source_or_workflow_sha_mismatch",
            ));
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), CiError> {
        let campaign = &self.campaign;
        let campaign_ok = !campaign.is_empty()
            && campaign.len() <= 64
            && campaign
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && campaign
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
        if !campaign_ok {
            return Err(identity("qualification.campaign", "invalid_campaign"));
        }
        let parts: Vec<&str> = self.repository.split('/').collect();
        let repository_ok = parts.len() == 2
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            });
        if !repository_ok {
            return Err(identity("qualification.repository", "invalid_repository"));
        }
        if !lower_hex_sha(&self.source_sha) || !lower_hex_sha(&self.workflow_sha) {
            return Err(identity("qualification.sha", "invalid_sha"));
        }
        if self.source_sha != self.workflow_sha || !self.ref_protected {
            return Err(identity(
                "qualification.provenance",
                "unprotected_or_unbound_workflow",
            ));
        }
        if !valid_branch_name(&self.default_branch)
            || self.git_ref != format!("refs/heads/{}", self.default_branch)
        {
            return Err(identity(
                "qualification.default_branch",
                "default_branch_mismatch",
            ));
        }
        if self.workflow_ref != format!("{}/{CI_WORKFLOW_PATH}@{}", self.repository, self.git_ref) {
            return Err(identity(
                "qualification.workflow_ref",
                "workflow_ref_mismatch",
            ));
        }
        if self.run_id == 0 || self.run_attempt == 0 {
            return Err(identity("qualification.run", "invalid_run_identity"));
        }
        match (self.phase.predecessor(), self.predecessor) {
            (None, None) => Ok(()),
            (None, Some(_)) => Err(identity(
                "qualification.predecessor",
                "unexpected_run_reference",
            )),
            (Some(_), None) => Err(identity(
                "qualification.predecessor",
                "missing_run_reference",
            )),
            (Some(_), Some(reference)) => reference.validate(),
        }
    }
}

pub(super) fn validate_source_delta(
    delta: &SourceDelta,
    expected_base: &str,
    expected_source: &str,
) -> Result<(), CiError> {
    if !delta.base_is_ancestor
        || delta.base_source_sha != expected_base
        || delta.source_sha != expected_source
        || delta.base_source_sha == delta.source_sha
        || delta.changed_paths.is_empty()
        || delta.changed_paths.len() > 256
        || delta
            .changed_paths
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(invalid("useful_source_delta_unbound_or_unordered"));
    }
    for path in &delta.changed_paths {
        if path.len() > 256 || normalize_posix_path(path)? != *path {
            return Err(invalid("useful_source_delta_path_invalid"));
        }
    }
    validate_digest(&delta.diff_digest)?;
    if delta.diff_digest != digest_b3(&canonical_bytes(&delta.changed_paths)?) {
        return Err(invalid("useful_source_delta_digest_mismatch"));
    }
    Ok(())
}

fn check_nesting(bytes: &[u8]) -> Result<(), CiError> {
    let (mut depth, mut quoted, mut escaped) = (0_usize, false, false);
    for byte in bytes {
        if quoted {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => quoted = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > MAX_NESTING {
                        return Err(invalid("json_too_deep"));
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

fn check_text(value: &str) -> Result<(), CiError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        Err(invalid("receipt_text_invalid"))
    } else {
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_source_sha(value: &str) -> Result<(), CiError> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(invalid("receipt_source_sha_invalid"))
    }
}

fn receipt_digest(receipt: &Receipt) -> Result<String, CiError> {
    Ok(digest_b3(&canonical_bytes(receipt)?))
}

/// The legacy `QualificationCacheAdmission::parse_bounded`: the bytes parse and the lineage is
/// internally consistent.
pub(super) fn parse_admission(bytes: &[u8]) -> Result<Admission, CiError> {
    if bytes.is_empty() || bytes.len() > MAX_RECEIPT_BYTES {
        return Err(invalid("artifact_size"));
    }
    check_nesting(bytes)?;
    let document: Admission =
        serde_json::from_slice(bytes).map_err(|_| invalid("malformed_admission"))?;
    node_shape(&document.predecessor, 1)?;
    Ok(document)
}

fn node_shape(node: &Node, depth: usize) -> Result<(), CiError> {
    if depth >= MAX_DEPTH {
        return Err(invalid("lineage_too_deep"));
    }
    node_metadata(node)?;
    receipt_shape(&node.receipt)?;
    if let Some(previous) = &node.previous {
        let link = node
            .receipt
            .predecessor
            .as_ref()
            .ok_or_else(|| invalid("missing_predecessor_link"))?;
        if link.run != previous.receipt.run
            || link.receipt_digest != receipt_digest(&previous.receipt)?
            || link.artifact_id != previous.artifact.id
            || link.artifact_digest != previous.artifact.digest
        {
            return Err(invalid("predecessor_receipt_digest_mismatch"));
        }
        node_shape(previous, depth + 1)?;
    } else if node.receipt.predecessor.is_some() {
        return Err(invalid("missing_predecessor_receipt"));
    }
    Ok(())
}

fn node_metadata(node: &Node) -> Result<(), CiError> {
    node.metadata.run.validate()?;
    node.producer.run.validate()?;
    node.receipt.run.validate()?;
    let artifact = &node.artifact;
    if artifact.id == 0
        || artifact.name != RECEIPT_ARTIFACT
        || artifact.expired
        || artifact.size_bytes == 0
        || artifact.size_bytes > MAX_RECEIPT_BYTES as u64
        || artifact.workflow_run_id == 0
        || !valid_sha256(&artifact.digest)
    {
        return Err(invalid("artifact_metadata_invalid"));
    }
    let (meta, producer) = (&node.metadata, &node.producer);
    for value in [
        &meta.repository,
        &meta.default_branch,
        &meta.git_ref,
        &meta.workflow_path_ref,
        &meta.workflow_ref,
        &meta.workflow_sha,
        &meta.head_sha,
        &meta.event,
        &meta.conclusion,
        &artifact.name,
        &artifact.workflow_head_branch,
        &artifact.workflow_head_sha,
        &producer.repository,
        &producer.default_branch,
        &producer.git_ref,
        &producer.workflow_ref,
        &producer.workflow_sha,
        &producer.source_sha,
    ] {
        check_text(value)?;
    }
    for sha in [
        &meta.workflow_sha,
        &meta.head_sha,
        &producer.workflow_sha,
        &producer.source_sha,
    ] {
        validate_source_sha(sha)?;
    }
    Ok(())
}

fn receipt_shape(receipt: &Receipt) -> Result<(), CiError> {
    if receipt.schema != 2 || receipt.lanes.len() > MAX_LANES || receipt.lanes.is_empty() {
        return Err(invalid("receipt_schema_or_lane_count"));
    }
    receipt.run.validate()?;
    validate_plan_id(&receipt.plan_id)?;
    let run_key = format!("r{}-a{}", receipt.run.run_id, receipt.run.run_attempt);
    if format!("plan-{run_key}") != receipt.plan_id {
        return Err(invalid("receipt_plan_id_run_mismatch"));
    }
    validate_digest(&receipt.configuration_digest)?;
    check_text(&receipt.campaign)?;
    validate_source_sha(&receipt.source_sha)?;
    if let Some(link) = &receipt.predecessor {
        link.run.validate()?;
        validate_digest(&link.receipt_digest)?;
        if link.artifact_id == 0 || !valid_sha256(&link.artifact_digest) {
            return Err(invalid("receipt_artifact_link_invalid"));
        }
    }
    match (receipt.phase, receipt.source_delta.as_ref()) {
        (Phase::UsefulDelta, Some(delta)) => {
            validate_source_delta(delta, &delta.base_source_sha, &receipt.source_sha)?;
        }
        (Phase::UsefulDelta, None) => return Err(invalid("missing_useful_source_delta")),
        (_, Some(_)) => return Err(invalid("unexpected_source_delta")),
        (_, None) => {}
    }
    for pair in receipt.lanes.windows(2) {
        if pair[0].matrix_key >= pair[1].matrix_key {
            return Err(invalid("lanes_not_sorted_unique"));
        }
    }
    Ok(())
}
