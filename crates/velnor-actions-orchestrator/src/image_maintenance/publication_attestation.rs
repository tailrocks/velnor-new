// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    ImageCandidateSourceClosure, serialize_image_candidate_source_closure,
};
use velnor_actions_mise::CheckDeadline;

use crate::{
    OrchestratorError,
    package_release_signer::{ImageAttestationIdentity, verify_image_file_attestation},
};

use super::{PublishImageRequest, authority};

pub(super) struct AttestationRun<'a> {
    pub caller_workflow_path: &'a str,
    pub caller_workflow_sha: &'a str,
    pub signer_workflow_path: &'a str,
    pub signer_workflow_sha: &'a str,
    pub source_sha: &'a str,
    pub event: &'a str,
    pub run_id: u64,
    pub run_attempt: u32,
}

pub(super) fn verify_job_image(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    run: &AttestationRun<'_>,
    source_files: &BTreeMap<String, Vec<u8>>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let archive_name = authority::archive_name(request.profile);
    let record_name = authority::record_name(request.profile);
    let mut expected_subjects = BTreeMap::from([
        (archive_name.to_owned(), candidate.archive.sha256.clone()),
        (record_name.to_owned(), candidate.record_sha256.clone()),
    ]);
    let closure = ImageCandidateSourceClosure::new(
        candidate.source_commit.clone(),
        source_files
            .iter()
            .map(|(path, bytes)| (path.clone(), super::authority::hex(&Sha256::digest(bytes))))
            .collect(),
    )
    .map_err(|_| crate::internal::internal("image_publication_closure_invalid"))?;
    let bytes = serialize_image_candidate_source_closure(&closure)
        .map_err(|_| crate::internal::internal("image_publication_closure_invalid"))?;
    expected_subjects.insert(
        "job-image-native-source-closure.json".to_owned(),
        super::authority::hex(&Sha256::digest(bytes)),
    );
    let identity = ImageAttestationIdentity {
        repository: authority::IMAGE_REPOSITORY,
        repository_id: authority::IMAGE_REPOSITORY_ID,
        caller_workflow_path: run.caller_workflow_path,
        caller_workflow_sha: run.caller_workflow_sha,
        signer_workflow_path: run.signer_workflow_path,
        signer_workflow_sha: run.signer_workflow_sha,
        signer_ref: "refs/heads/main",
        source_sha: run.source_sha,
        source_ref: "refs/heads/main",
        event: run.event,
        run_id: run.run_id,
        run_attempt: run.run_attempt,
    };
    verify_image_file_attestation(
        &request.archive_path,
        &identity,
        &expected_subjects,
        &[],
        deadline,
    )?;
    verify_image_file_attestation(
        &request.record_path,
        &identity,
        &expected_subjects,
        &[],
        deadline,
    )
}
