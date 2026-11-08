// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{FixtureProfile, QUALIFICATION_ATTEST_WORKFLOW, REPOSITORY, oci, workflow_uri};

pub(crate) fn qualification_proof(
    selected: FixtureProfile,
    source: &str,
    lock_sha256: &str,
    producer_run_attempt: u32,
    selected_image: &oci::Layout,
    record_sha256: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut images = serde_json::Map::new();
    for profile in [
        FixtureProfile::Runner,
        FixtureProfile::Dind,
        FixtureProfile::ResourceProbe,
    ] {
        let layout = if profile == selected {
            None
        } else {
            Some(oci::create_runner_for(profile, source)?)
        };
        let selected_layout = layout.as_ref().unwrap_or(selected_image);
        let digest = if profile == selected {
            record_sha256.to_owned()
        } else {
            let source_files = super::source_files(profile)?;
            let source_inputs = source_files
                .iter()
                .map(|(path, bytes)| (path.clone(), super::hex(&Sha256::digest(bytes))))
                .collect();
            let archive_sha = super::hex(&Sha256::digest(&selected_layout.archive));
            let record = super::make_record(super::RecordInput {
                profile,
                source,
                authority_sha: source,
                inputs: &source_inputs,
                archive_sha: &archive_sha,
                image: selected_layout,
                source_files: &source_files,
                run_attempt: producer_run_attempt,
            })?;
            super::hex(&Sha256::digest(record))
        };
        images.insert(
            profile.name().to_owned(),
            image_value(selected_layout, &digest)?,
        );
    }
    Ok(serde_json::to_vec(&json!({
        "schema":"velnor.runner-image-qualification.v2",
        "source_commit":source,
        "source_ref":"refs/heads/main",
        "runner_lock_sha256":lock_sha256,
        "producer_run_id":selected.ci_run_id(),
        "producer_run_attempt":producer_run_attempt,
        "qualifier_run_id":selected.qualification_run_id(),
        "qualifier_run_attempt":1,
        "qualifier_job_id":selected.qualification_job_id(),
        "images":images
    }))?)
}

fn image_value(layout: &oci::Layout, record_sha256: &str) -> Result<Value, Box<dyn Error>> {
    Ok(json!({
        "record_sha256":record_sha256,
        "provenance":layout.provenance.as_ref().ok_or("runner provenance missing")?,
        "archive":{"sha256":hex(&Sha256::digest(&layout.archive)),"size_bytes":layout.archive.len()},
        "oci":&layout.identity
    }))
}

pub(crate) fn qualification_attestation(
    profile: FixtureProfile,
    source: &str,
    proof_sha256: &str,
) -> Value {
    let caller_uri = workflow_uri(".github/workflows/runner-image-qualification.yml");
    let signer_uri = workflow_uri(QUALIFICATION_ATTEST_WORKFLOW);
    let invocation = format!(
        "https://github.com/{REPOSITORY}/actions/runs/{}/attempts/1",
        profile.qualification_run_id(),
    );
    json!([{"verificationResult":{"statement":{"predicateType":"https://slsa.dev/provenance/v1","subject":[{"name":"RUNNER_IMAGE_QUALIFICATION.json","digest":{"sha256":proof_sha256}}],
        "predicate":{"buildDefinition":{"internalParameters":{"github":{"event_name":"push"}}}}},
        "signature":{"certificate":{"issuer":"https://token.actions.githubusercontent.com","subjectAlternativeName":signer_uri,
            "sourceRepositoryURI":format!("https://github.com/{REPOSITORY}"),"sourceRepositoryOwnerURI":"https://github.com/tailrocks","sourceRepositoryIdentifier":"1390620900",
            "sourceRepositoryDigest":source,"sourceRepositoryRef":"refs/heads/main","buildSignerURI":signer_uri,"buildSignerDigest":source,
            "buildConfigURI":caller_uri,"buildConfigDigest":source,"buildTrigger":"push","runInvocationURI":invocation,"runnerEnvironment":"github-hosted"}}}}])
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}
