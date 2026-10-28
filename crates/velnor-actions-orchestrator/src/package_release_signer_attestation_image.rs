// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{
    AttestationCommandIdentity, AttestationExpectation, source_attestation_token,
    validate_output_with_additional_names, verify_one,
};

pub(crate) struct ImageAttestationIdentity<'a> {
    pub(crate) repository: &'a str,
    pub(crate) repository_id: u64,
    pub(crate) caller_workflow_path: &'a str,
    pub(crate) caller_workflow_sha: &'a str,
    pub(crate) signer_workflow_path: &'a str,
    pub(crate) signer_workflow_sha: &'a str,
    pub(crate) source_sha: &'a str,
    pub(crate) source_ref: &'a str,
    pub(crate) event: &'a str,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
}

pub(crate) fn verify_image_file_attestation(
    path: &std::path::Path,
    identity: &ImageAttestationIdentity<'_>,
    expected_subjects: &BTreeMap<String, String>,
    additional_subject_names: &[&str],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let source_token = source_attestation_token()?;
    let caller_workflow_uri = format!(
        "https://github.com/{}/{}@{}",
        identity.repository, identity.caller_workflow_path, identity.source_ref
    );
    let signer_workflow_uri = format!(
        "https://github.com/{}/{}@{}",
        identity.repository, identity.signer_workflow_path, identity.source_ref
    );
    let signer_workflow = format!(
        "github.com/{}/{}@{}",
        identity.repository, identity.signer_workflow_path, identity.source_ref
    );
    let repository_uri = format!("https://github.com/{}", identity.repository);
    let owner_uri = format!(
        "https://github.com/{}",
        identity
            .repository
            .split_once('/')
            .ok_or_else(|| internal("package_attestation_repository_invalid"))?
            .0
    );
    let expectation = AttestationExpectation {
        repository_uri: &repository_uri,
        owner_uri: &owner_uri,
        caller_workflow_uri: &caller_workflow_uri,
        caller_workflow_sha: identity.caller_workflow_sha,
        signer_workflow_uri: &signer_workflow_uri,
        signer_workflow_sha: identity.signer_workflow_sha,
        source_sha: identity.source_sha,
        source_ref: identity.source_ref,
        run_id: identity.run_id,
        run_attempt: identity.run_attempt,
        repository_id: identity.repository_id.to_string(),
        trigger: identity.event,
    };
    let command_identity = AttestationCommandIdentity {
        repository: identity.repository,
        workflow_uri: &signer_workflow,
        workflow_sha: identity.signer_workflow_sha,
        source_sha: identity.source_sha,
        source_ref: identity.source_ref,
        source_token: &source_token,
    };
    let output = verify_one(path, &command_identity, deadline)?;
    let _ = validate_output_with_additional_names(
        &output,
        expected_subjects,
        additional_subject_names,
        &expectation,
    )?;
    Ok(())
}
