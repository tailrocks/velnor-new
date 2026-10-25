use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    time::Duration,
};

use serde::Deserialize;
use sha2::Digest;
use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};

use super::{PackageReleaseSigningIdentity, VerifiedSignedPackage};
use crate::{OrchestratorError, internal::internal};

const PREDICATE: &str = "https://slsa.dev/provenance/v1";
const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const VERIFY_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) struct AttestationCommandIdentity<'a> {
    pub(super) repository: &'a str,
    pub(super) workflow_uri: &'a str,
    pub(super) workflow_sha: &'a str,
    pub(super) source_sha: &'a str,
    pub(super) source_ref: &'a str,
    pub(super) source_token: &'a str,
}

pub(super) fn verify_release_attestation(
    signed: &VerifiedSignedPackage,
    expected: &PackageReleaseSigningIdentity,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let receipt = signed.receipt();
    let subjects = expected_subjects(receipt)?;
    let source_token = source_attestation_token()?;
    if subjects.len() != receipt.evidence.attested_subject_names.len() {
        return Err(internal("package_attestation_subject_inventory_invalid"));
    }

    let workflow_uri = format!(
        "https://github.com/{}/{}@{}",
        expected.repository, expected.workflow_path, expected.source_ref
    );
    let signer_workflow = signer_workflow_argument(expected);
    let repository_uri = format!("https://github.com/{}", expected.repository);
    let owner_uri = format!(
        "https://github.com/{}",
        expected
            .repository
            .split_once('/')
            .ok_or_else(|| internal("package_attestation_repository_invalid"))?
            .0
    );
    let invocation_uri = format!(
        "https://github.com/{}/actions/runs/{}/attempts/{}",
        expected.repository, expected.run_id, expected.run_attempt
    );
    let expectation = AttestationExpectation {
        repository_uri: &repository_uri,
        owner_uri: &owner_uri,
        workflow_uri: &workflow_uri,
        workflow_sha: &expected.workflow_sha,
        source_sha: &expected.source_sha,
        source_ref: &expected.source_ref,
        run_id: expected.run_id,
        run_attempt: expected.run_attempt,
        repository_id: expected.repository_id.to_string(),
        trigger: expected.event.as_str(),
        invocation_uri: &invocation_uri,
    };

    for (name, digest) in &subjects {
        let path = signed.directory().join(name);
        let identity = AttestationCommandIdentity {
            repository: &expected.repository,
            workflow_uri: &signer_workflow,
            workflow_sha: &expected.workflow_sha,
            source_sha: &expected.source_sha,
            source_ref: &expected.source_ref,
            source_token: &source_token,
        };
        let output = verify_one(&path, &identity, deadline)?;
        validate_output(&output, &subjects, &expectation, name, digest)?;
    }
    Ok(())
}

fn verify_one(
    path: &std::path::Path,
    identity: &AttestationCommandIdentity<'_>,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    verify_one_with_bundle(path, None, identity, deadline)
}

fn verify_one_with_bundle(
    path: &std::path::Path,
    bundle: Option<&std::path::Path>,
    identity: &AttestationCommandIdentity<'_>,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut args = vec![
        OsString::from("attestation"),
        OsString::from("verify"),
        path.as_os_str().to_owned(),
    ];
    if let Some(bundle) = bundle {
        args.extend([OsString::from("--bundle"), bundle.as_os_str().to_owned()]);
    }
    args.extend([
        OsString::from("--repo"),
        OsString::from(identity.repository),
        OsString::from("--signer-workflow"),
        OsString::from(identity.workflow_uri),
        OsString::from("--signer-digest"),
        OsString::from(identity.workflow_sha),
        OsString::from("--source-digest"),
        OsString::from(identity.source_sha),
        OsString::from("--source-ref"),
        OsString::from(identity.source_ref),
        OsString::from("--predicate-type"),
        OsString::from(PREDICATE),
        OsString::from("--cert-oidc-issuer"),
        OsString::from(OIDC_ISSUER),
        OsString::from("--deny-self-hosted-runners"),
        OsString::from("--limit"),
        OsString::from("10"),
        OsString::from("--format"),
        OsString::from("json"),
    ]);
    let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("package_attestation_gh_unavailable"))?;
    let timeout = deadline
        .remaining()
        .map(|remaining| remaining.min(VERIFY_TIMEOUT))
        .map_err(|_| internal("package_verification_deadline"))?;
    let output = exec
        .command_with_github_token(&ToolCatalog::pinned(), identity.source_token)
        .and_then(|command| command.run_bounded(VERIFY_OUTPUT_LIMIT, timeout))
        .map_err(|_| internal("package_attestation_verification_failed"))?;
    if !output.success || output.stdout.is_empty() {
        return Err(internal("package_attestation_verification_failed"));
    }
    Ok(output.stdout)
}

fn source_attestation_token() -> Result<String, OrchestratorError> {
    validate_source_attestation_token(env::var("GH_TOKEN").ok(), env::var("VELNOR_TAP_TOKEN").ok())
}

fn validate_source_attestation_token(
    source_token: Option<String>,
    tap_token: Option<String>,
) -> Result<String, OrchestratorError> {
    let source_token = source_token
        .filter(|token| !token.is_empty())
        .ok_or_else(|| internal("package_attestation_source_token_missing"))?;
    if tap_token
        .filter(|token| !token.is_empty())
        .is_some_and(|tap_token| tap_token == source_token)
    {
        return Err(internal("package_attestation_token_role_collision"));
    }
    Ok(source_token)
}

fn signer_workflow_argument(identity: &PackageReleaseSigningIdentity) -> String {
    format!(
        "github.com/{}/{}@{}",
        identity.repository, identity.workflow_path, identity.source_ref
    )
}

#[derive(Debug, Deserialize)]
struct VerifiedAttestation {
    #[serde(rename = "verificationResult")]
    verification_result: VerificationResult,
}

#[derive(Debug, Deserialize)]
struct VerificationResult {
    statement: Statement,
    signature: SignatureResult,
}

#[derive(Debug, Deserialize)]
struct Statement {
    #[serde(rename = "predicateType")]
    predicate_type: String,
    subject: Vec<Subject>,
    predicate: SlsaPredicate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlsaPredicate {
    build_definition: SlsaBuildDefinition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlsaBuildDefinition {
    internal_parameters: SlsaInternalParameters,
}

#[derive(Debug, Deserialize)]
struct SlsaInternalParameters {
    github: SlsaGitHubParameters,
}

#[derive(Debug, Deserialize)]
struct SlsaGitHubParameters {
    event_name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Subject {
    name: String,
    digest: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct SignatureResult {
    certificate: Certificate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Certificate {
    issuer: String,
    subject_alternative_name: String,
    #[serde(rename = "sourceRepositoryURI")]
    source_repository_uri: String,
    #[serde(rename = "sourceRepositoryOwnerURI")]
    source_repository_owner_uri: String,
    source_repository_identifier: String,
    source_repository_digest: String,
    source_repository_ref: String,
    #[serde(rename = "buildSignerURI")]
    build_signer_uri: String,
    build_signer_digest: String,
    #[serde(rename = "buildConfigURI")]
    build_config_uri: String,
    build_config_digest: String,
    build_trigger: String,
    #[serde(rename = "runInvocationURI")]
    run_invocation_uri: String,
    runner_environment: String,
}

struct AttestationExpectation<'a> {
    repository_uri: &'a str,
    owner_uri: &'a str,
    workflow_uri: &'a str,
    workflow_sha: &'a str,
    source_sha: &'a str,
    source_ref: &'a str,
    run_id: u64,
    run_attempt: u32,
    repository_id: String,
    trigger: &'a str,
    invocation_uri: &'a str,
}

fn validate_output(
    bytes: &[u8],
    expected_subjects: &BTreeMap<String, String>,
    expected: &AttestationExpectation<'_>,
    requested_name: &str,
    requested_digest: &str,
) -> Result<(), OrchestratorError> {
    let attestations: Vec<VerifiedAttestation> = serde_json::from_slice(bytes)
        .map_err(|_| internal("package_attestation_output_invalid"))?;
    if attestations.len() != 1 {
        return Err(internal("package_attestation_ambiguous"));
    }
    let verified = &attestations[0].verification_result;
    if verified.statement.predicate_type != PREDICATE {
        return Err(internal("package_attestation_predicate_mismatch"));
    }
    if verified
        .statement
        .predicate
        .build_definition
        .internal_parameters
        .github
        .event_name
        != expected.trigger
    {
        return Err(internal("package_attestation_event_mismatch"));
    }
    let mut found = BTreeMap::new();
    for subject in &verified.statement.subject {
        let digest = subject
            .digest
            .get("sha256")
            .ok_or_else(|| internal("package_attestation_subject_digest_invalid"))?;
        if subject.digest.len() != 1 || found.insert(subject.name.clone(), digest.clone()).is_some()
        {
            return Err(internal("package_attestation_subject_inventory_invalid"));
        }
    }
    if &found != expected_subjects
        || found.get(requested_name).map(String::as_str) != Some(requested_digest)
    {
        return Err(internal("package_attestation_subject_inventory_mismatch"));
    }
    let certificate = &verified.signature.certificate;
    if certificate.issuer != OIDC_ISSUER
        || certificate.subject_alternative_name != expected.workflow_uri
        || certificate.source_repository_uri != expected.repository_uri
        || certificate.source_repository_owner_uri != expected.owner_uri
        || certificate.source_repository_identifier != expected.repository_id
        || certificate.source_repository_digest != expected.source_sha
        || certificate.source_repository_ref != expected.source_ref
        || certificate.build_signer_uri != expected.workflow_uri
        || certificate.build_signer_digest != expected.workflow_sha
        || certificate.build_config_uri != expected.workflow_uri
        || certificate.build_config_digest != expected.workflow_sha
        || certificate.build_trigger != expected.trigger
        || certificate.run_invocation_uri != expected.invocation_uri
        || certificate.runner_environment != "github-hosted"
    {
        return Err(internal("package_attestation_identity_mismatch"));
    }
    let expected_invocation = format!(
        "https://github.com/{}/actions/runs/{}/attempts/{}",
        expected
            .repository_uri
            .strip_prefix("https://github.com/")
            .ok_or_else(|| internal("package_attestation_repository_invalid"))?,
        expected.run_id,
        expected.run_attempt
    );
    if certificate.run_invocation_uri != expected_invocation
        || certificate.run_invocation_uri != expected.invocation_uri
    {
        return Err(internal("package_attestation_run_identity_mismatch"));
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in sha2::Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[path = "package_release_signer_attestation_sdk.rs"]
mod sdk;
#[path = "package_release_signer_attestation_subjects.rs"]
mod subjects;

pub(crate) use sdk::{SdkManifestAttestationExpectation, verify_published_manifest_attestation};
use subjects::expected_subjects;
