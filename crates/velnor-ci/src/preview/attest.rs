// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use serde_json::Value;

use super::json::{Field, Res, is_hex, require};
use super::{
    AUTHORITY_WORKFLOW, OWNER, OWNER_ID, PUBLICATION_AUTHORITY_SHA, REPOSITORY, REPOSITORY_ID,
};
use crate::error::CiError;

/// The publication workflow commit that signs previews: `repository`, `path` and `ref`
/// are fixed, and `sha` is a full commit hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authority {
    pub sha: String,
}

impl Authority {
    /// Port of `staging_authority`: the pinned authority of `source_preview.rs`.
    pub fn pinned() -> Res<Self> {
        Self::checked(PUBLICATION_AUTHORITY_SHA)
    }

    /// Port of `checked_authority`.
    pub fn checked(sha: &str) -> Res<Self> {
        require(is_hex(sha, 40))?;
        Ok(Self {
            sha: sha.to_owned(),
        })
    }

    /// Port of `preview_authority` of `..._source_preview_authority.py`: the running
    /// workflow, given as the four `AUTHORITY_*` variables, is the publication workflow
    /// at a full commit.
    pub fn from_env(repository: &str, path: &str, git_ref: &str, sha: &str) -> Res<Self> {
        require(repository == REPOSITORY && path == AUTHORITY_WORKFLOW)?;
        require(is_hex(sha, 40) && git_ref == format!("{repository}/{path}@{sha}"))?;
        Self::checked(sha)
    }

    #[must_use]
    pub fn reference(&self) -> String {
        format!("{REPOSITORY}/{AUTHORITY_WORKFLOW}@{}", self.sha)
    }
}

/// What the signed receipt must say, and which assets it must cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub source_sha: String,
    pub signer_workflow_path: String,
    pub signer_run_id: u64,
    pub signer_run_attempt: u64,
    pub receipt_sha256: String,
    pub linux: (String, String),
    pub darwin: (String, String),
}

impl Subject {
    pub fn from_preview(preview: &Value) -> Res<Self> {
        let text = |key: &str| preview.at(key).text().to_owned();
        let number = |key: &str| {
            preview
                .at(key)
                .as_u64()
                .ok_or_else(|| CiError::new("source preview subject"))
        };
        Ok(Self {
            source_sha: text("source_sha"),
            signer_workflow_path: text("signer_workflow_path"),
            signer_run_id: number("signer_run_id")?,
            signer_run_attempt: number("signer_run_attempt")?,
            receipt_sha256: text("receipt_sha256"),
            linux: (text("linux_payload_name"), text("linux_payload_sha256")),
            darwin: (text("darwin_payload_name"), text("darwin_payload_sha256")),
        })
    }

    #[must_use]
    pub fn source_ref(&self) -> String {
        format!("refs/tags/sdk-preview-{}", self.source_sha)
    }
}

/// Runs `gh attestation verify` for a receipt and its bundle and returns the parsed
/// output. An implementation checks the `gh` binary against the pinned hashes first.
pub trait Attestor: std::fmt::Debug {
    fn verify(
        &self,
        receipt: &[u8],
        bundle: &[u8],
        subject: &Subject,
        authority: &Authority,
        deadline: Instant,
    ) -> Res<Value>;
}

/// Port of `_certificate_matches` with the public policy: every field of the Sigstore
/// certificate must be exactly the one the publication workflow's run would produce.
#[must_use]
pub fn certificate_matches(certificate: &Value, subject: &Subject, authority: &Authority) -> bool {
    let signer = format!("https://github.com/{}", authority.reference());
    let source_ref = subject.source_ref();
    let repository_uri = format!("https://github.com/{REPOSITORY}");
    let expected = [
        (
            "issuer",
            "https://token.actions.githubusercontent.com".to_owned(),
        ),
        ("subjectAlternativeName", signer.clone()),
        ("buildSignerURI", signer),
        ("buildSignerDigest", authority.sha.clone()),
        (
            "buildConfigURI",
            format!(
                "{repository_uri}/{}@{source_ref}",
                subject.signer_workflow_path
            ),
        ),
        ("buildConfigDigest", subject.source_sha.clone()),
        ("sourceRepositoryURI", repository_uri.clone()),
        ("sourceRepositoryIdentifier", REPOSITORY_ID.to_string()),
        (
            "sourceRepositoryOwnerURI",
            format!("https://github.com/{OWNER}"),
        ),
        ("sourceRepositoryOwnerIdentifier", OWNER_ID.to_owned()),
        ("sourceRepositoryDigest", subject.source_sha.clone()),
        ("sourceRepositoryRef", source_ref),
        ("buildTrigger", "push".to_owned()),
        ("runnerEnvironment", "github-hosted".to_owned()),
        (
            "runInvocationURI",
            format!(
                "{repository_uri}/actions/runs/{}/attempts/{}",
                subject.signer_run_id, subject.signer_run_attempt
            ),
        ),
    ];
    authority.sha != subject.source_sha
        && certificate.is_object()
        && expected
            .iter()
            .all(|(key, value)| certificate.at(key).is_s(value))
}

/// The checks of `_verify_attestation` that follow the `gh` run: exactly one verified
/// attestation, a matching certificate, SLSA provenance, and three subjects that are the
/// receipt and both payloads, each once, with the digests of the record.
pub fn check_verified(verified: &Value, subject: &Subject, authority: &Authority) -> Res<()> {
    let list = verified
        .as_array()
        .ok_or_else(|| CiError::new("source preview attestation"))?;
    require(list.len() == 1)?;
    let result = list[0].at("verificationResult");
    let certificate = result.at("signature").at("certificate");
    require(certificate_matches(certificate, subject, authority))?;
    let statement = result.at("statement");
    require(
        statement
            .at("predicateType")
            .is_s("https://slsa.dev/provenance/v1"),
    )?;
    let subjects = statement.at("subject").as_array();
    require(subjects.is_some_and(|s| s.len() == 3))?;
    let subjects = subjects.map_or(&[][..], Vec::as_slice);
    for (name, digest) in [
        ("SDK_PREVIEW_RECEIPT.json", &subject.receipt_sha256),
        (subject.linux.0.as_str(), &subject.linux.1),
        (subject.darwin.0.as_str(), &subject.darwin.1),
    ] {
        let matches: Vec<&Value> = subjects
            .iter()
            .filter(|s| s.at("name").is_s(name))
            .collect();
        require(matches.len() == 1)?;
        let want = serde_json::json!({ "sha256": digest });
        require(matches[0].at("digest") == &want)?;
    }
    Ok(())
}
