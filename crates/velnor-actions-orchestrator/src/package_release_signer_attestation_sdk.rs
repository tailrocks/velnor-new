// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::CheckDeadline;

use super::{
    AttestationCommandIdentity, OrchestratorError, source_attestation_token, verify_one_with_bundle,
};
use crate::internal::internal;

pub(crate) struct SdkManifestAttestationExpectation<'a> {
    pub(crate) repository: &'a str,
    pub(crate) repository_id: u64,
    pub(crate) owner_id: u64,
    pub(crate) source_sha: &'a str,
    pub(crate) manifest_sha256: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedManifestAttestation {
    pub(crate) source_sha: String,
    pub(crate) workflow_sha: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) event: String,
    subject_sha256: String,
}

pub(crate) fn verify_published_manifest_attestation(
    path: &std::path::Path,
    bundle_path: &std::path::Path,
    expectation: &SdkManifestAttestationExpectation<'_>,
    deadline: CheckDeadline,
) -> Result<VerifiedManifestAttestation, OrchestratorError> {
    if expectation.repository != "tailrocks/velnor-new"
        || expectation.repository_id != 1_390_620_900
        || expectation.owner_id != 78_806_509
        || !is_sha(expectation.source_sha)
        || !is_sha256(expectation.manifest_sha256)
    {
        return Err(internal("sdk_manifest_attestation_expectation_invalid"));
    }
    let source_ref = "refs/heads/main";
    let signer_path = ".github/workflows/product-release-generator.yml";
    let workflow_uri = format!(
        "github.com/{}/{signer_path}@{source_ref}",
        expectation.repository
    );
    let source_token = source_attestation_token()?;
    let identity = AttestationCommandIdentity {
        repository: expectation.repository,
        workflow_uri: &workflow_uri,
        workflow_sha: expectation.source_sha,
        source_sha: expectation.source_sha,
        source_ref,
        source_token: &source_token,
    };
    let raw = verify_one_with_bundle(path, Some(bundle_path), &identity, deadline)?;
    parse_verified_manifest_attestation(
        &raw,
        expectation.repository,
        expectation.repository_id,
        expectation.owner_id,
        expectation.source_sha,
        expectation.manifest_sha256,
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AttestedLinuxGenerator {
    version: String,
    sha256: String,
    source_sha: String,
    run_id: u64,
    run_attempt: u32,
}

#[cfg(test)]
pub(crate) fn fixture_attested_generator() -> AttestedLinuxGenerator {
    AttestedLinuxGenerator {
        version: "0.1.72".into(),
        sha256: "1".repeat(64),
        source_sha: "f1041f322c54cd7edfdef06afd307799eeaa801a".into(),
        run_id: 1,
        run_attempt: 1,
    }
}

#[path = "package_release_signer_attestation_sdk_start.rs"]
mod start;
#[cfg(test)]
pub(crate) use start::fixture_measured_tools;
pub(crate) use start::{
    AdmittedStart, MeasuredReleaseTools, measure_release_tools, record_admitted_start,
};
#[cfg(test)]
use start::{derive_attested_tool_expectations, match_attested_tool_expectations};

pub(crate) fn verify_published_linux_binary_attestation(
    path: &std::path::Path,
    bundle_path: &std::path::Path,
    manifest_bytes: &[u8],
    manifest_attestation: &VerifiedManifestAttestation,
    deadline: CheckDeadline,
) -> Result<AttestedLinuxGenerator, OrchestratorError> {
    let manifest = bound_release_manifest(manifest_bytes, manifest_attestation)?;
    if manifest.repository != "tailrocks/velnor-new"
        || manifest.commit != manifest_attestation.source_sha
        || manifest_attestation.workflow_sha != manifest.commit
        || manifest_attestation.event != "workflow_dispatch"
    {
        return Err(internal("sdk_binary_manifest_mismatch"));
    }
    let target = manifest
        .record_for_target("x86_64-unknown-linux-gnu")
        .ok_or_else(|| internal("sdk_binary_target_missing"))?;
    let name = format!(
        "velnor-actions-{}-x86_64-unknown-linux-gnu",
        manifest.version
    );
    if path.file_name().and_then(std::ffi::OsStr::to_str) != Some(name.as_str()) {
        return Err(internal("sdk_binary_name_invalid"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| internal("sdk_binary_path_invalid"))?;
    let bytes = match crate::safe_read::read_repo_bytes(parent, &name, 128 * 1024 * 1024)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
        crate::safe_read::RepoBytes::Absent => return Err(internal("sdk_binary_missing")),
    };
    if crate::generator::sha256_hex(&bytes) != target.sha256 {
        return Err(internal("sdk_binary_digest_mismatch"));
    }
    let source_token = source_attestation_token()?;
    let identity = AttestationCommandIdentity {
        repository: "tailrocks/velnor-new",
        workflow_uri: "github.com/tailrocks/velnor-new/.github/workflows/product-release-generator.yml@refs/heads/main",
        workflow_sha: &manifest.commit,
        source_sha: &manifest.commit,
        source_ref: "refs/heads/main",
        source_token: &source_token,
    };
    let raw = verify_one_with_bundle(path, Some(bundle_path), &identity, deadline)?;
    let binary = parse_verified_release_attestation(
        &raw,
        "tailrocks/velnor-new",
        1_390_620_900,
        78_806_509,
        &manifest.commit,
        &SubjectExpectation {
            name: &name,
            sha256: &target.sha256,
            binary: true,
        },
    )?;
    if !same_release_run(&binary, manifest_attestation) {
        return Err(internal("sdk_binary_attestation_mismatch"));
    }
    Ok(AttestedLinuxGenerator {
        version: manifest.version.clone(),
        sha256: target.sha256.clone(),
        source_sha: binary.source_sha,
        run_id: binary.run_id,
        run_attempt: binary.run_attempt,
    })
}

fn bound_release_manifest(
    bytes: &[u8],
    attestation: &VerifiedManifestAttestation,
) -> Result<velnor_actions_contract::ReleaseManifest, OrchestratorError> {
    if bytes.len() > 1024 * 1024
        || crate::generator::sha256_hex(bytes) != attestation.subject_sha256
    {
        return Err(internal("sdk_binary_manifest_mismatch"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| internal("sdk_binary_manifest_invalid"))?;
    let manifest = velnor_actions_contract::ReleaseManifest::parse_json_with_limit(
        text,
        "release-manifest.json",
        1024 * 1024,
    )
    .map_err(|_| internal("sdk_binary_manifest_invalid"))?;
    manifest
        .validate("release-manifest.json")
        .map_err(|_| internal("sdk_binary_manifest_invalid"))?;
    Ok(manifest)
}

fn same_release_run(
    left: &VerifiedManifestAttestation,
    right: &VerifiedManifestAttestation,
) -> bool {
    left.source_sha == right.source_sha
        && left.workflow_sha == right.workflow_sha
        && left.run_id == right.run_id
        && left.run_attempt == right.run_attempt
        && left.event == right.event
}

#[path = "package_release_signer_attestation_sdk_parse.rs"]
mod parse;
pub(crate) use parse::parse_verified_manifest_attestation;
use parse::{SubjectExpectation, is_sha, is_sha256, parse_verified_release_attestation};

#[cfg(test)]
#[path = "package_release_signer_attestation_sdk_tests.rs"]
mod subject_tests;
