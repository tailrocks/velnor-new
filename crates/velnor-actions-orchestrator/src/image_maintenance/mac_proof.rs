// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use velnor_actions_contract::runner_image_manifest::{
    RunnerImageManifest,
    mac_attestation::{
        MAC_COMPANION_REPOSITORY_ID, MAC_COMPANION_RUNNER_ID, MacCompanionAuthority,
        MacPublicAuthority, MacSdkIdentity,
        parse_mac_qualification_attestation, receipt_matches_manifest,
    },
};
use velnor_actions_mise::{
    COSIGN_COMMAND_OUTPUT_LIMIT, COSIGN_COMMAND_TIMEOUT, CheckDeadline, CosignCertificateIdentity,
};
use x509_cert::{
    Certificate,
    der::{Decode, asn1::Utf8StringRef},
};

use crate::{
    OrchestratorError, internal::internal, package_release_signer::cosign::acquire_cosign,
};

const TRUSTED_ROOT: &[u8] = include_bytes!("sigstore-trusted-root.json");
const TRUSTED_ROOT_SHA256: &str =
    "6494e21ea73fa7ee769f85f57d5a3e6a08725eae1e38c755fc3517c9e6bc0b66";

#[derive(Debug, Clone, Copy)]
struct MacPrivatePolicy<'a> {
    pub workflow_path: &'a str,
    pub workflow_ref: &'a str,
    pub workflow_sha: &'a str,
    pub runner_id: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct VerifyMacQualificationRequest<'a> {
    pub envelope: &'a [u8],
    pub bundle: &'a [u8],
    pub public: &'a MacPublicAuthority,
    pub sdk: &'a MacSdkIdentity,
    pub manifest_bytes: &'a [u8],
    pub manifest: &'a RunnerImageManifest,
    pub workflow_sha: &'a str,
    pub runner_temp: &'a Path,
    pub deadline: CheckDeadline,
}

pub fn verify_mac_qualification(
    request: &VerifyMacQualificationRequest<'_>,
) -> Result<(), OrchestratorError> {
    let policy = MacPrivatePolicy {
        workflow_path: ".github/workflows/qualify-runner-family.yml",
        workflow_ref: "refs/heads/main",
        workflow_sha: request.workflow_sha,
        runner_id: MAC_COMPANION_RUNNER_ID,
    };
    verify_with_policy(request, &policy)
}

fn verify_with_policy(
    request: &VerifyMacQualificationRequest<'_>,
    policy: &MacPrivatePolicy<'_>,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("mac_proof_invalid");
    let (envelope, receipt) =
        parse_mac_qualification_attestation(request.envelope).map_err(|_| invalid())?;
    let private = &envelope.companion;
    if &envelope.public_authority != request.public
        || &envelope.sdk != request.sdk
        || private.workflow_path != policy.workflow_path
        || private.workflow_ref != policy.workflow_ref
        || private.workflow_sha != policy.workflow_sha
        || private.runner_id != policy.runner_id
        || policy.workflow_sha.len() != 40
        || request.manifest.source_commit != request.public.source_commit
        || !receipt_matches_manifest(&receipt, request.manifest, request.manifest_bytes)
    {
        return Err(invalid());
    }
    let parsed_manifest: RunnerImageManifest = serde_json::from_value(
        velnor_actions_contract::strict_json::parse_strict_json_bytes(
            request.manifest_bytes,
            1024 * 1024,
        )
        .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    if serde_json::to_value(parsed_manifest).map_err(|_| invalid())?
        != serde_json::to_value(request.manifest).map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    let cert = certificate_from_bundle(request.bundle)?;
    verify_certificate_claims(&cert, private, policy)?;
    if hex(&Sha256::digest(TRUSTED_ROOT)) != TRUSTED_ROOT_SHA256 {
        return Err(invalid());
    }
    let subject = sealed_file(request.envelope)?;
    let bundle_file = sealed_file(request.bundle)?;
    let trusted_root = sealed_file(TRUSTED_ROOT)?;
    let subject_path = sealed_path(&subject);
    let bundle_path = sealed_path(&bundle_file);
    let trusted_root_path = sealed_path(&trusted_root);
    let cosign = acquire_cosign(request.runner_temp, request.deadline)?;
    let identity = CosignCertificateIdentity::new(
        &private.repository,
        &private.workflow_path,
        &private.workflow_ref,
        policy.workflow_sha,
        "workflow_dispatch",
    )
    .map_err(|_| invalid())?;
    let command = cosign
        .verify_blob_command_with_trusted_root(
            &subject_path,
            &bundle_path,
            &identity,
            &trusted_root_path,
        )
        .map_err(|_| invalid())?;
    let timeout = request
        .deadline
        .remaining()
        .map_err(|_| invalid())?
        .min(COSIGN_COMMAND_TIMEOUT);
    let result = command
        .run_bounded(COSIGN_COMMAND_OUTPUT_LIMIT, timeout)
        .map_err(|_| invalid())?;
    if !result.success {
        return Err(invalid());
    }
    Ok(())
}

fn certificate_from_bundle(bundle_bytes: &[u8]) -> Result<Certificate, OrchestratorError> {
    let invalid = || internal("mac_proof_invalid");
    let bundle = velnor_actions_contract::strict_json::parse_strict_json_bytes(
        bundle_bytes,
        8 * 1024 * 1024,
    )
    .map_err(|_| invalid())?;
    let encoded = bundle
        .pointer("/verificationMaterial/certificate/rawBytes")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid)?;
    let certificate = STANDARD.decode(encoded).map_err(|_| invalid())?;
    if STANDARD.encode(&certificate) != encoded || certificate.len() > 64 * 1024 {
        return Err(invalid());
    }
    Certificate::from_der(&certificate).map_err(|_| invalid())
}

fn verify_certificate_claims(
    cert: &Certificate,
    private: &MacCompanionAuthority,
    policy: &MacPrivatePolicy<'_>,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("mac_proof_invalid");
    let identity = format!(
        "https://github.com/{}/{}@{}",
        private.repository, private.workflow_path, private.workflow_ref
    );
    let invocation = format!(
        "https://github.com/{}/actions/runs/{}/attempts/{}",
        private.repository, private.run_id, private.run_attempt
    );
    let repo = format!("https://github.com/{}", private.repository);
    let repository_id = MAC_COMPANION_REPOSITORY_ID.to_string();
    let checks = [
        (8, "https://token.actions.githubusercontent.com"),
        (9, identity.as_str()),
        (10, policy.workflow_sha),
        (11, "github-hosted"),
        (12, repo.as_str()),
        (13, policy.workflow_sha),
        (14, policy.workflow_ref),
        (15, repository_id.as_str()),
        (16, "https://github.com/donbeave"),
        (17, "139017"),
        (18, identity.as_str()),
        (19, policy.workflow_sha),
        (20, "workflow_dispatch"),
        (21, invocation.as_str()),
        (22, "private"),
    ];
    let extensions = cert
        .tbs_certificate
        .extensions
        .as_ref()
        .ok_or_else(invalid)?;
    for (number, expected) in checks {
        let oid = format!("1.3.6.1.4.1.57264.1.{number}");
        let mut matches = extensions
            .iter()
            .filter(|extension| extension.extn_id.to_string() == oid);
        let extension = matches.next().ok_or_else(invalid)?;
        if matches.next().is_some()
            || Utf8StringRef::from_der(extension.extn_value.as_bytes())
                .map_err(|_| invalid())?
                .as_str()
                != expected
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(target_os = "linux")]
fn sealed_file(bytes: &[u8]) -> Result<std::fs::File, OrchestratorError> {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use std::io::Write;
    let fd = memfd_create("velnor-mac-proof", MemfdFlags::ALLOW_SEALING)
        .map_err(|_| internal("mac_proof_sealed_file"))?;
    let mut file = std::fs::File::from(fd);
    file.write_all(bytes)
        .map_err(|_| internal("mac_proof_sealed_file"))?;
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .map_err(|_| internal("mac_proof_sealed_file"))?;
    Ok(file)
}

#[cfg(target_os = "linux")]
fn sealed_path(file: &std::fs::File) -> std::path::PathBuf {
    use std::os::fd::AsRawFd;
    format!("/proc/self/fd/{}", file.as_raw_fd()).into()
}

#[cfg(not(target_os = "linux"))]
fn sealed_file(_bytes: &[u8]) -> Result<std::fs::File, OrchestratorError> {
    Err(internal("mac_proof_platform_unsupported"))
}

#[cfg(not(target_os = "linux"))]
fn sealed_path(_file: &std::fs::File) -> std::path::PathBuf {
    std::path::PathBuf::new()
}

#[cfg(test)]
#[path = "mac_proof_tests.rs"]
mod tests;
