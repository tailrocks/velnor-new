// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    ImageCandidateSourceClosure, serialize_image_candidate_source_closure,
};

use super::{FixtureProfile, REPOSITORY, hex};

#[derive(Clone, Copy)]
pub(crate) struct AttestationInput<'a> {
    pub(crate) profile: FixtureProfile,
    pub(crate) source: &'a str,
    pub(crate) authority_sha: &'a str,
    pub(crate) archive_sha: &'a str,
    pub(crate) record_sha: &'a str,
    pub(crate) source_files: &'a BTreeMap<String, Vec<u8>>,
    pub(crate) run_attempt: u32,
}

pub(crate) fn attestation(input: AttestationInput<'_>) -> Result<Value, Box<dyn Error>> {
    let AttestationInput {
        profile,
        source,
        authority_sha,
        archive_sha,
        record_sha,
        source_files,
        run_attempt,
        ..
    } = input;
    let caller_path = profile.workflow_path();
    let caller_uri = workflow_uri(caller_path);
    let signer_uri = caller_uri.clone();
    let invocation = format!(
        "https://github.com/{REPOSITORY}/actions/runs/{}/attempts/{run_attempt}",
        profile.run_id(),
    );
    let mut subjects = base_subjects(profile, archive_sha, record_sha);
    if profile == FixtureProfile::Job {
        add_job_closure(&mut subjects, source, source_files)?;
    }
    let subject_values = subjects
        .iter()
        .map(|(name, digest)| json!({"name":name,"digest":{"sha256":digest}}))
        .collect::<Vec<_>>();
    let source_digest = match profile {
        FixtureProfile::Job => source,
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            authority_sha
        }
    };
    Ok(
        json!([{"verificationResult":{"statement":{"predicateType":"https://slsa.dev/provenance/v1","subject":subject_values,
        "predicate":{"buildDefinition":{"internalParameters":{"github":{"event_name":"push"}}}}},
        "signature":{"certificate":{"issuer":"https://token.actions.githubusercontent.com","subjectAlternativeName":signer_uri,
            "sourceRepositoryURI":format!("https://github.com/{REPOSITORY}"),"sourceRepositoryOwnerURI":"https://github.com/tailrocks","sourceRepositoryIdentifier":"1390620900",
            "sourceRepositoryDigest":source_digest,"sourceRepositoryRef":"refs/heads/main","buildSignerURI":signer_uri,"buildSignerDigest":authority_sha,
            "buildConfigURI":caller_uri,"buildConfigDigest":authority_sha,"buildTrigger":"push","runInvocationURI":invocation,"runnerEnvironment":"github-hosted"}}}}]),
    )
}

fn base_subjects(profile: FixtureProfile, archive: &str, record: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (profile.archive_name().to_owned(), archive.to_owned()),
        (profile.record_name().to_owned(), record.to_owned()),
    ])
}

fn add_job_closure(
    subjects: &mut BTreeMap<String, String>,
    source: &str,
    source_files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), Box<dyn Error>> {
    let inputs = source_files
        .iter()
        .map(|(path, bytes)| (path.clone(), hex(&Sha256::digest(bytes))))
        .collect();
    let closure = ImageCandidateSourceClosure::new(source.to_owned(), inputs)?;
    subjects.insert(
        "job-image-native-source-closure.json".to_owned(),
        hex(&Sha256::digest(serialize_image_candidate_source_closure(
            &closure,
        )?)),
    );
    Ok(())
}

pub(crate) fn workflow_uri(path: &str) -> String {
    format!("https://github.com/{REPOSITORY}/{path}@refs/heads/main")
}
