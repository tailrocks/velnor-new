use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_actions_mise::CheckDeadline;

use super::{
    AttestationCommandIdentity, OIDC_ISSUER, OrchestratorError, PREDICATE, SignatureResult,
    Subject, source_attestation_token, verify_one_with_bundle,
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

pub(crate) fn parse_verified_manifest_attestation(
    raw: &[u8],
    repository: &str,
    repository_id: u64,
    owner_id: u64,
    source_sha: &str,
    manifest_sha256: &str,
) -> Result<VerifiedManifestAttestation, OrchestratorError> {
    if repository != "tailrocks/velnor-new"
        || repository_id != 1_390_620_900
        || owner_id != 78_806_509
        || !is_sha(source_sha)
        || !is_sha256(manifest_sha256)
    {
        return Err(internal("sdk_manifest_attestation_expectation_invalid"));
    }
    let attestations: Vec<SdkManifestAttestation> = serde_json::from_slice(raw)
        .map_err(|_| internal("sdk_manifest_attestation_output_invalid"))?;
    if attestations.len() != 1 {
        return Err(internal("sdk_manifest_attestation_ambiguous"));
    }
    let result = &attestations[0].verification_result;
    let run = validate_sdk_manifest_identity(
        result,
        repository,
        repository_id,
        owner_id,
        source_sha,
        manifest_sha256,
    )?;
    Ok(VerifiedManifestAttestation {
        source_sha: source_sha.to_owned(),
        workflow_sha: source_sha.to_owned(),
        run_id: run.0,
        run_attempt: run.1,
        event: result
            .statement
            .predicate
            .build_definition
            .internal_parameters
            .github
            .event_name
            .clone(),
    })
}

fn validate_sdk_manifest_identity(
    result: &SdkVerificationResult,
    repository: &str,
    repository_id: u64,
    owner_id: u64,
    source_sha: &str,
    manifest_sha256: &str,
) -> Result<(u64, u32), OrchestratorError> {
    let source_ref = "refs/heads/main";
    let signer_path = ".github/workflows/product-release-generator.yml";
    let signer_uri = format!("https://github.com/{repository}/{signer_path}@{source_ref}");
    let statement = &result.statement;
    let cert = &result.signature.certificate;
    let github = &statement
        .predicate
        .build_definition
        .internal_parameters
        .github;
    let external = &statement
        .predicate
        .build_definition
        .external_parameters
        .workflow;
    let build = &statement.predicate.build_definition;
    let dependencies = &build.resolved_dependencies;
    let run = parse_run_invocation(&cert.run_invocation_uri, repository)?;
    if statement.predicate_type != PREDICATE
        || build.build_type != "https://actions.github.io/buildtypes/workflow/v1"
        || dependencies.len() != 1
        || dependencies[0].uri != format!("git+https://github.com/{repository}@{source_ref}")
        || dependencies[0].digest.len() != 1
        || dependencies[0].digest.get("gitCommit").map(String::as_str) != Some(source_sha)
        || statement.predicate.run_details.metadata.invocation_id != cert.run_invocation_uri
        || statement.predicate.run_details.builder.id != signer_uri
        || statement.subject.len() != 1
        || statement.subject[0].name != "release-manifest.json"
        || statement.subject[0].digest.len() != 1
        || statement.subject[0]
            .digest
            .get("sha256")
            .map(String::as_str)
            != Some(manifest_sha256)
        || github.event_name != "workflow_dispatch"
        || github.repository_id != repository_id.to_string()
        || github.repository_owner_id != owner_id.to_string()
        || github.runner_environment != "github-hosted"
        || external.path != ".github/workflows/product-release.yml"
        || external.ref_name != source_ref
        || external.repository != format!("https://github.com/{repository}")
        || cert.issuer != OIDC_ISSUER
        || cert.subject_alternative_name != signer_uri
        || cert.source_repository_uri != format!("https://github.com/{repository}")
        || cert.source_repository_owner_uri != "https://github.com/tailrocks"
        || cert.source_repository_identifier != repository_id.to_string()
        || cert.source_repository_digest != source_sha
        || cert.source_repository_ref != source_ref
        || cert.build_signer_uri != signer_uri
        || cert.build_signer_digest != source_sha
        || cert.build_config_uri
            != format!(
                "https://github.com/{repository}/.github/workflows/product-release.yml@{source_ref}"
            )
        || cert.build_config_digest != source_sha
        || cert.build_trigger != "workflow_dispatch"
        || cert.runner_environment != "github-hosted"
    {
        return Err(internal("sdk_manifest_attestation_identity_mismatch"));
    }
    Ok(run)
}

#[derive(Deserialize)]
struct SdkManifestAttestation {
    #[serde(rename = "verificationResult")]
    verification_result: SdkVerificationResult,
}

#[derive(Deserialize)]
struct SdkVerificationResult {
    statement: SdkStatement,
    signature: SignatureResult,
}

#[derive(Deserialize)]
struct SdkStatement {
    #[serde(rename = "predicateType")]
    predicate_type: String,
    subject: Vec<Subject>,
    predicate: SdkPredicate,
}

#[derive(Deserialize)]
struct SdkPredicate {
    #[serde(rename = "buildDefinition")]
    build_definition: SdkBuildDefinition,
    #[serde(rename = "runDetails")]
    run_details: SdkRunDetails,
}

#[derive(Deserialize)]
struct SdkBuildDefinition {
    #[serde(rename = "buildType")]
    build_type: String,
    #[serde(rename = "externalParameters")]
    external_parameters: SdkExternalParameters,
    #[serde(rename = "internalParameters")]
    internal_parameters: SdkInternalParameters,
    #[serde(rename = "resolvedDependencies")]
    resolved_dependencies: Vec<SdkResolvedDependency>,
}

#[derive(Deserialize)]
struct SdkResolvedDependency {
    uri: String,
    digest: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct SdkExternalParameters {
    workflow: SdkWorkflow,
}

#[derive(Deserialize)]
struct SdkWorkflow {
    path: String,
    #[serde(rename = "ref")]
    ref_name: String,
    repository: String,
}

#[derive(Deserialize)]
struct SdkInternalParameters {
    github: SdkGitHubParameters,
}

#[derive(Deserialize)]
struct SdkGitHubParameters {
    event_name: String,
    repository_id: String,
    repository_owner_id: String,
    runner_environment: String,
}

#[derive(Deserialize)]
struct SdkRunDetails {
    builder: SdkBuilder,
    metadata: SdkRunMetadata,
}

#[derive(Deserialize)]
struct SdkBuilder {
    id: String,
}

#[derive(Deserialize)]
struct SdkRunMetadata {
    #[serde(rename = "invocationId")]
    invocation_id: String,
}

fn parse_run_invocation(value: &str, repository: &str) -> Result<(u64, u32), OrchestratorError> {
    let prefix = format!("https://github.com/{repository}/actions/runs/");
    let remainder = value
        .strip_prefix(&prefix)
        .ok_or_else(|| internal("sdk_manifest_attestation_invocation_invalid"))?;
    let (run, attempt) = remainder
        .split_once("/attempts/")
        .ok_or_else(|| internal("sdk_manifest_attestation_invocation_invalid"))?;
    if run.is_empty()
        || attempt.is_empty()
        || run.bytes().any(|byte| !byte.is_ascii_digit())
        || attempt.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return Err(internal("sdk_manifest_attestation_invocation_invalid"));
    }
    let run_id = run
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("sdk_manifest_attestation_invocation_invalid"))?;
    let run_attempt = attempt
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("sdk_manifest_attestation_invocation_invalid"))?;
    Ok((run_id, run_attempt))
}

fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
