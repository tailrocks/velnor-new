// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

pub const RUNTIME_AUTHORITY_SHA: Option<&str> = None;
pub const PUBLICATION_AUTHORITY_SHA: &str = "3a3c641b531a69b7e4142b61aecf536007bb3f2e";

const REPOSITORY: &str = "tailrocks/velnor-new";
const SIGNER_WORKFLOW: &str = ".github/workflows/product-release-generator.yml";
const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
const QUALIFICATION_WORKFLOW: &str = ".github/workflows/source-qualification.yml";
const MAX_RECEIPT_BYTES: usize = 65_536;
const MAX_PAYLOAD_BYTES: u64 = 268_435_456;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreviewReceipt {
    pub schema: u32,
    pub repository: String,
    pub version: String,
    pub source_sha: String,
    pub tag: String,
    pub release_id: u64,
    pub source: SourcePreviewRun,
    pub draft: SourcePreviewRun,
    pub signer: SourcePreviewRun,
    pub ci: SourcePreviewRun,
    pub qualification: SourcePreviewRun,
    pub candidate_source_artifact_id: u64,
    pub targets: SourcePreviewTargets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreviewRun {
    pub workflow_path: String,
    pub workflow_ref: String,
    pub workflow_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreviewPayload {
    pub target: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreviewProducer {
    pub producer_run_id: u64,
    pub producer_run_attempt: u32,
    pub payload_artifact_id: u64,
    pub payload_archive_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreviewTargets {
    pub linux: SourcePreviewTarget,
    pub darwin: SourcePreviewTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreviewTarget {
    pub producer: SourcePreviewProducer,
    pub candidate_payload_artifact_id: u64,
    pub qualification_job_id: u64,
    pub payload: SourcePreviewPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreviewLock {
    pub receipt: SourcePreviewReceipt,
    pub receipt_sha256: String,
    pub bundle_sha256: String,
    pub publisher_run_attempt: u32,
    pub publisher_job_id: u64,
}

impl SourcePreviewLock {
    pub fn validate(&self, candidate_sha: &str) -> Result<(), crate::ContractError> {
        self.validate_shape()?;
        if !lower_hex(candidate_sha, 40) || self.receipt.source_sha == candidate_sha {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn validate_shape(&self) -> Result<(), crate::ContractError> {
        let receipt = &self.receipt;
        if receipt.schema != 3
            || receipt.repository != REPOSITORY
            || !valid_version(&receipt.version)
            || !lower_hex(&receipt.source_sha, 40)
            || receipt.tag != format!("sdk-preview-{}", receipt.source_sha)
            || receipt.release_id == 0
            || [&receipt.source, &receipt.draft].iter().any(|run| {
                !valid_run(
                    run,
                    SIGNER_WORKFLOW,
                    &receipt.source_sha,
                    &format!("refs/tags/{}", receipt.tag),
                ) || run.run_id != receipt.signer.run_id
            })
            || receipt.source.run_attempt > receipt.draft.run_attempt
            || receipt.draft.run_attempt > receipt.signer.run_attempt
            || receipt.signer.run_attempt > self.publisher_run_attempt
            || self.publisher_run_attempt > 51
            || self.publisher_job_id == 0
            || receipt.candidate_source_artifact_id == 0
            || !valid_target(&receipt.targets.linux, "x86_64-unknown-linux-gnu")
            || !valid_target(&receipt.targets.darwin, "aarch64-apple-darwin")
            || !distinct_artifacts(receipt)
            || receipt.targets.linux.qualification_job_id
                == receipt.targets.darwin.qualification_job_id
            || !valid_run(
                &receipt.signer,
                SIGNER_WORKFLOW,
                &receipt.source_sha,
                &format!("refs/tags/{}", receipt.tag),
            )
            || !valid_run(
                &receipt.ci,
                CI_WORKFLOW,
                &receipt.source_sha,
                "refs/heads/main",
            )
            || !valid_run(
                &receipt.qualification,
                QUALIFICATION_WORKFLOW,
                &receipt.source_sha,
                "refs/heads/main",
            )
            || !lower_hex(&self.receipt_sha256, 64)
            || !lower_hex(&self.bundle_sha256, 64)
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn validate_receipt_bytes(
        &self,
        bytes: &[u8],
        candidate_sha: &str,
    ) -> Result<(), crate::ContractError> {
        self.validate(candidate_sha)?;
        if bytes.len() > MAX_RECEIPT_BYTES
            || crate::canonical::sha256_hex(bytes) != self.receipt_sha256
            || crate::canonical_json_bytes(&self.receipt)? != bytes
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn distinct_artifacts(receipt: &SourcePreviewReceipt) -> bool {
    let ids = [
        receipt.candidate_source_artifact_id,
        receipt.targets.linux.producer.payload_artifact_id,
        receipt.targets.darwin.producer.payload_artifact_id,
        receipt.targets.linux.candidate_payload_artifact_id,
        receipt.targets.darwin.candidate_payload_artifact_id,
    ];
    ids.iter()
        .enumerate()
        .all(|(index, id)| !ids[index + 1..].contains(id))
}

fn valid_target(target: &SourcePreviewTarget, expected: &str) -> bool {
    let producer = &target.producer;
    let payload = &target.payload;
    producer.producer_run_id != 0
        && producer.producer_run_attempt != 0
        && producer.payload_artifact_id != 0
        && lower_hex(&producer.payload_archive_sha256, 64)
        && target.candidate_payload_artifact_id != 0
        && target.qualification_job_id != 0
        && payload.target == expected
        && payload.name == format!("velnor-actions-{expected}")
        && payload.size != 0
        && payload.size <= MAX_PAYLOAD_BYTES
        && lower_hex(&payload.sha256, 64)
}

fn valid_run(run: &SourcePreviewRun, path: &str, source: &str, git_ref: &str) -> bool {
    run.workflow_path == path
        && run.workflow_ref == format!("{REPOSITORY}/{path}@{git_ref}")
        && run.workflow_sha == source
        && run.run_id != 0
        && run.run_attempt != 0
}

fn valid_version(version: &str) -> bool {
    let mut parts = version.split('.');
    (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

fn lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid() -> crate::ContractError {
    crate::ContractError::identity("source_preview", "invalid_preview_identity")
}
