// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Write},
    path::Path,
};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};

use super::{
    AttestationCommandIdentity, VERIFY_OUTPUT_LIMIT, VERIFY_TIMEOUT, verify_one_with_bundle,
};
use crate::{OrchestratorError, internal::internal};

const HEX: &[u8; 16] = b"0123456789abcdef";

#[derive(Deserialize)]
struct Page {
    attestations: Vec<Attestation>,
}

#[derive(Deserialize)]
struct Attestation {
    bundle: Option<Value>,
    bundle_url: Option<String>,
}

pub(super) fn verify_all(
    path: &Path,
    identity: &AttestationCommandIdentity<'_>,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let digest = file_digest(path, deadline)?;
    let endpoint = format!(
        "repos/{}/attestations/sha256:{digest}?per_page=100&predicate_type=https%3A%2F%2Fslsa.dev%2Fprovenance%2Fv1",
        identity.repository
    );
    let args = ["api", "--paginate", "--slurp", &endpoint]
        .into_iter()
        .map(OsString::from)
        .collect();
    let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("package_attestation_gh_unavailable"))?;
    let timeout = deadline
        .remaining()
        .map(|remaining| remaining.min(VERIFY_TIMEOUT))
        .map_err(|_| internal("package_verification_deadline"))?;
    let output = exec
        .command_with_github_token(&ToolCatalog::pinned(), identity.source_token)
        .and_then(|command| command.run_bounded(VERIFY_OUTPUT_LIMIT, timeout))
        .map_err(|_| internal("package_attestation_lookup_failed"))?;
    if !output.success || output.stdout.is_empty() {
        return Err(internal("package_attestation_lookup_failed"));
    }
    let pages: Vec<Page> = serde_json::from_slice(&output.stdout)
        .map_err(|_| internal("package_attestation_lookup_invalid"))?;
    if pages.is_empty() {
        return Err(internal("package_attestation_lookup_invalid"));
    }
    let mut seen = BTreeSet::new();
    let mut bundle_bytes = 0_usize;
    let mut written_bytes = 0_usize;
    let mut bundles = Vec::new();
    for page in pages {
        for attestation in page.attestations {
            let raw = bundle_bytes_for(attestation, deadline)?;
            bundle_bytes = bundle_bytes
                .checked_add(raw.len().saturating_add(1))
                .filter(|size| *size <= VERIFY_OUTPUT_LIMIT)
                .ok_or_else(|| internal("package_attestation_bundle_limit"))?;
            let value: Value = serde_json::from_slice(&raw)
                .map_err(|_| internal("package_attestation_bundle_invalid"))?;
            let bundle = serde_json::to_vec(&value)
                .map_err(|_| internal("package_attestation_bundle_invalid"))?;
            written_bytes = written_bytes
                .checked_add(bundle.len().saturating_add(1))
                .filter(|size| *size <= VERIFY_OUTPUT_LIMIT)
                .ok_or_else(|| internal("package_attestation_bundle_limit"))?;
            if !seen.insert(super::sha256(&bundle)) {
                return Err(internal("package_attestation_ambiguous"));
            }
            bundles.push(bundle);
        }
    }
    if seen.is_empty() {
        return Err(internal("package_attestation_ambiguous"));
    }
    verify_bundles(path, identity, deadline, bundles)
}

fn verify_bundles(
    path: &Path,
    identity: &AttestationCommandIdentity<'_>,
    deadline: CheckDeadline,
    bundles: Vec<Vec<u8>>,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut verified = Vec::<Value>::new();
    let mut verified_bytes = 0_usize;
    for bundle in bundles {
        let mut file = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .map_err(|_| internal("package_attestation_bundle_temporary"))?;
        file.write_all(&bundle)
            .map_err(|_| internal("package_attestation_bundle_temporary"))?;
        let output = verify_one_with_bundle(
            path,
            file.path(),
            identity,
            deadline,
            super::VerificationPolicy::Repository,
        )?;
        verified_bytes = verified_bytes
            .checked_add(output.len())
            .filter(|size| *size <= VERIFY_OUTPUT_LIMIT)
            .ok_or_else(|| internal("package_attestation_bundle_limit"))?;
        let entries: Vec<Value> = serde_json::from_slice(&output)
            .map_err(|_| internal("package_attestation_output_invalid"))?;
        if entries.is_empty() {
            return Err(internal("package_attestation_ambiguous"));
        }
        verified.extend(entries);
    }
    bounded_verified_output(&verified)
}

fn bounded_verified_output(verified: &[Value]) -> Result<Vec<u8>, OrchestratorError> {
    let output =
        serde_json::to_vec(verified).map_err(|_| internal("package_attestation_output_invalid"))?;
    if output.len() > VERIFY_OUTPUT_LIMIT {
        return Err(internal("package_attestation_bundle_limit"));
    }
    Ok(output)
}

fn bundle_bytes_for(
    attestation: Attestation,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    if let Some(bundle) = attestation.bundle {
        return serde_json::to_vec(&bundle)
            .map_err(|_| internal("package_attestation_bundle_invalid"));
    }
    let url = attestation
        .bundle_url
        .ok_or_else(|| internal("package_attestation_bundle_invalid"))?;
    crate::api::github_evidence_http::fetch_attestation_bundle(&url, VERIFY_OUTPUT_LIMIT, deadline)
}

fn file_digest(path: &Path, deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    let mut file = File::open(path).map_err(|_| internal("package_attestation_subject_read"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        deadline
            .remaining()
            .map_err(|_| internal("package_verification_deadline"))?;
        let size = file
            .read(&mut buffer)
            .map_err(|_| internal("package_attestation_subject_read"))?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    let mut digest = String::with_capacity(64);
    for byte in hash.finalize() {
        digest.push(char::from(HEX[usize::from(byte >> 4)]));
        digest.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::{VERIFY_OUTPUT_LIMIT, bounded_verified_output};
    use serde_json::Value;

    #[test]
    fn combined_verified_output_includes_array_and_string_delimiters_in_limit() {
        let exact = Value::String("x".repeat(VERIFY_OUTPUT_LIMIT - 4));
        assert_eq!(
            bounded_verified_output(&[exact])
                .expect("exact bound")
                .len(),
            VERIFY_OUTPUT_LIMIT
        );
        let oversized = Value::String("x".repeat(VERIFY_OUTPUT_LIMIT - 3));
        let error =
            bounded_verified_output(&[oversized]).expect_err("array delimiters exceed bound");
        assert!(
            error
                .to_string()
                .contains("package_attestation_bundle_limit")
        );
    }
}
