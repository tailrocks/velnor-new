// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(crate) struct AdmittedStart<'a, J, G, R> {
    pub(crate) journal: &'a J,
    pub(crate) github: &'a G,
    pub(crate) registry: &'a R,
    pub(crate) intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    pub(crate) request: &'a crate::crate_release_github::GitHubAdmissionRequest,
    pub(crate) admission: &'a crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    pub(crate) source: &'a std::path::Path,
    pub(crate) workflow_root: &'a std::path::Path,
    pub(crate) generator: &'a AttestedLinuxGenerator,
}

pub(crate) fn record_admitted_start<
    J: crate::crate_release_journal::JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
>(
    input: &AdmittedStart<'_, J, G, R>,
) -> Result<String, OrchestratorError> {
    if input.admission.schema != 1
        || input.admission.intent != *input.intent
        || input.admission.github.request != *input.request
    {
        return Err(internal("crate_release_start_admission_mismatch"));
    }
    crate::crate_release_github::authorize_github_candidate(
        input.request,
        &input.admission.github,
        input.github,
    )?;
    let observed = crate::crate_release_controller::prepare_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
    )?;
    crate::crate_release_controller::verify_observed_packages(&observed, input.admission)?;
    let receipt = derive_start_receipt(input, &observed)?;
    let prior_history = verified_prior_chain(input, &receipt)?;
    crate::crate_release_github::authorize_github_candidate(
        input.request,
        &input.admission.github,
        input.github,
    )?;
    let result = crate::crate_release_journal::record_source_bound_start(
        input.journal,
        &receipt.identity,
        &receipt,
        input.source,
    )?;
    if verified_prior_chain(input, &receipt)? != prior_history {
        return Err(internal("crate_release_previous_changed"));
    }
    if crate::crate_release_controller::prepare_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
    )? != observed
    {
        return Err(internal("crate_release_start_state_changed"));
    }
    crate::crate_release_github::authorize_github_candidate(
        input.request,
        &input.admission.github,
        input.github,
    )?;
    Ok(result)
}

fn verified_prior_chain<J, G, R>(
    input: &AdmittedStart<'_, J, G, R>,
    receipt: &crate::crate_release_journal::JournalReceipt,
) -> Result<Vec<crate::crate_release_journal::PriorHistoryEvidence>, OrchestratorError>
where
    J: crate::crate_release_journal::JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: crate::crate_release_github::GitHubAdmissionRead,
{
    let chain =
        crate::crate_release_journal::inspect_prior_chain(input.journal, receipt, input.source)?;
    for prior in &chain {
        let previous = &prior.start.receipt;
        let intent = crate::crate_release_runtime::read_intent_at_workflow(
            input.workflow_root,
            &previous.identity.workflow_sha,
        )?;
        let bytes = velnor_actions_contract::canonical_json_bytes(&intent)
            .map_err(|_| internal("crate_release_previous_invalid"))?;
        let expected_predecessor = intent.journal_predecessor.as_ref().map(|item| {
            crate::crate_release_journal::JournalIdentity {
                repository: previous.identity.repository.clone(),
                source_sha: previous.identity.source_sha.clone(),
                workflow_sha: item.workflow_sha.clone(),
                run_id: item.run_id,
                run_attempt: item.run_attempt,
            }
        });
        if crate::generator::sha256_hex(&bytes) != previous.intent_sha256
            || crate::crate_release_journal::release_base_intent_sha256(&intent)?
                != previous.base_intent_sha256
            || expected_predecessor != previous.predecessor
        {
            return Err(internal("crate_release_previous_invalid"));
        }
        crate::crate_release_github::verify_predecessor_authority(
            input.github,
            input.request,
            previous,
            &intent,
        )?;
    }
    Ok(chain)
}

fn derive_start_receipt<
    J: crate::crate_release_journal::JournalIo,
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
>(
    input: &AdmittedStart<'_, J, G, R>,
    observed: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<crate::crate_release_journal::JournalReceipt, OrchestratorError> {
    use crate::crate_release_journal::{
        JournalAuthentication, JournalIdentity, JournalOwner, JournalOwnerKind, JournalPackage,
        JournalReceipt, JournalRegistry,
    };
    let identity = JournalIdentity {
        repository: input.request.repository.clone(),
        workflow_sha: input.request.workflow_sha.clone(),
        source_sha: input.request.source_sha.clone(),
        run_id: input.request.run_id,
        run_attempt: input.request.run_attempt,
    };
    let authentication = match input.intent.authentication {
        velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing => {
            JournalAuthentication::TrustedPublishing
        }
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken => {
            JournalAuthentication::BootstrapToken
        }
    };
    let packages = observed
        .packages
        .iter()
        .map(|package| {
            let owners = match &package.owners {
                crate::crate_release_registry::CrateRegistryOwnership::NewCrate => crate::crate_release_journal::JournalOwnership::NewCrate,
                crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } => {
                    let owners = owners.iter().map(|owner| {
                        let kind = match owner.kind.as_str() {
                            "user" => JournalOwnerKind::User,
                            "team" => JournalOwnerKind::Team,
                            _ => return Err(internal("crate_release_start_owner_invalid")),
                        };
                        Ok(JournalOwner { id: owner.id, login: owner.login.clone(), kind })
                    }).collect::<Result<Vec<_>, OrchestratorError>>()?;
                    crate::crate_release_journal::JournalOwnership::Existing { owners }
                }
            };
            Ok(JournalPackage {
                name: package.source.name.clone(),
                version: package.source.version.clone(),
                manifest: package.source.manifest.clone(),
                tag: package.tag.clone(),
                owners,
            })
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    let intent_bytes = velnor_actions_contract::canonical_json_bytes(input.intent)
        .map_err(|_| internal("crate_release_start_intent_invalid"))?;
    Ok(JournalReceipt {
        predecessor: input.intent.journal_predecessor.as_ref().map(|previous| {
            crate::crate_release_journal::JournalIdentity {
                repository: identity.repository.clone(),
                workflow_sha: previous.workflow_sha.clone(),
                source_sha: identity.source_sha.clone(),
                run_id: previous.run_id,
                run_attempt: previous.run_attempt,
            }
        }),
        identity: identity.clone(),
        intent_sha256: crate::generator::sha256_hex(&intent_bytes),
        base_intent_sha256: crate::crate_release_journal::release_base_intent_sha256(input.intent)?,
        registry: JournalRegistry::CratesIo,
        generator_version: input.intent.generator_version.clone(),
        release_plz_version: input.intent.release_plz_version.clone(),
        rust_version: input.intent.rust_version.clone(),
        expected_tools: derive_attested_tool_expectations(input.intent, input.generator)?,
        authentication,
        required_check_id: observed.github.required_check_id,
        trusted_reviewer_id: input.request.trusted_reviewer_id,
        workflow_run_url: format!(
            "https://github.com/{}/actions/runs/{}",
            input.request.repository, input.request.run_id
        ),
        source_files: crate::journal_source_inventory(input.source, &input.request.source_sha)?,
        packages,
        semver_evidence: input.admission.semver_evidence.clone(),
    })
}

fn match_attested_tool_expectations(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    receipt: &crate::crate_release_journal::JournalReceipt,
    generator: &AttestedLinuxGenerator,
) -> Result<(), OrchestratorError> {
    let expected = derive_attested_tool_expectations(intent, generator)?;
    if receipt.generator_version != intent.generator_version
        || receipt.release_plz_version != intent.release_plz_version
        || receipt.rust_version != intent.rust_version
        || receipt.expected_tools != expected
    {
        return Err(internal("crate_release_tool_expectations_mismatch"));
    }
    Ok(())
}

fn derive_attested_tool_expectations(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    generator: &AttestedLinuxGenerator,
) -> Result<crate::crate_release_journal::JournalToolPins, OrchestratorError> {
    intent
        .validate()
        .map_err(|_| internal("crate_release_tool_expectations_invalid"))?;
    let release_plz = velnor_actions_mise::catalog::qualified_release_plz(
        &intent.release_plz_version,
        &intent.runner,
    )
    .ok_or_else(|| internal("crate_release_tool_expectations_invalid"))?;
    let rust = velnor_actions_mise::catalog::rust_channel_manifest_sha256(&intent.rust_version)
        .ok_or_else(|| internal("crate_release_tool_expectations_invalid"))?;
    if intent.runner != "ubuntu-24.04"
        || generator.version != intent.generator_version
        || generator.source_sha != intent.generator_source_sha
    {
        return Err(internal("crate_release_tool_expectations_mismatch"));
    }
    Ok(crate::crate_release_journal::JournalToolPins {
        generator_sha256: generator.sha256.clone(),
        release_plz_sha256: release_plz.executable_sha256.to_owned(),
        rust_manifest_sha256: rust.to_owned(),
    })
}

pub(crate) struct MeasuredReleaseTools(crate::crate_release_journal::JournalToolPins);

#[cfg(test)]
pub(crate) fn fixture_measured_tools(
    pins: crate::crate_release_journal::JournalToolPins,
) -> MeasuredReleaseTools {
    MeasuredReleaseTools(pins)
}

impl MeasuredReleaseTools {
    pub(crate) fn into_pins(self) -> crate::crate_release_journal::JournalToolPins {
        self.0
    }
}

pub(crate) fn measure_release_tools(
    runner_temp: &std::path::Path,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    receipt: &crate::crate_release_journal::JournalReceipt,
    generator: &AttestedLinuxGenerator,
) -> Result<MeasuredReleaseTools, OrchestratorError> {
    match_attested_tool_expectations(intent, receipt, generator)?;
    let read_hash = |relative: &str, limit| match crate::safe_read::read_repo_bytes(
        runner_temp,
        relative,
        limit,
    )? {
        crate::safe_read::RepoBytes::Bytes(bytes) => Ok(crate::generator::sha256_hex(&bytes)),
        crate::safe_read::RepoBytes::Absent => {
            Err(internal("crate_release_tool_measurement_missing"))
        }
    };
    let generator_name = format!(
        "velnor/bootstrap/velnor-actions-{}-x86_64-unknown-linux-gnu",
        intent.generator_version
    );
    let observed = crate::crate_release_journal::JournalToolPins {
        generator_sha256: read_hash(&generator_name, 128 * 1024 * 1024)?,
        release_plz_sha256: read_hash("velnor-release-plz/release-plz", 128 * 1024 * 1024)?,
        rust_manifest_sha256: read_hash("velnor/rust-manifest/channel.toml", 16 * 1024 * 1024)?,
    };
    if observed != receipt.expected_tools {
        return Err(internal("crate_release_tool_measurement_mismatch"));
    }
    Ok(MeasuredReleaseTools(observed))
}

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

pub(crate) fn parse_verified_manifest_attestation(
    raw: &[u8],
    repository: &str,
    repository_id: u64,
    owner_id: u64,
    source_sha: &str,
    manifest_sha256: &str,
) -> Result<VerifiedManifestAttestation, OrchestratorError> {
    parse_verified_release_attestation(
        raw,
        repository,
        repository_id,
        owner_id,
        source_sha,
        &SubjectExpectation {
            name: "release-manifest.json",
            sha256: manifest_sha256,
            binary: false,
        },
    )
}

struct SubjectExpectation<'a> {
    name: &'a str,
    sha256: &'a str,
    binary: bool,
}

fn parse_verified_release_attestation(
    raw: &[u8],
    repository: &str,
    repository_id: u64,
    owner_id: u64,
    source_sha: &str,
    subject: &SubjectExpectation<'_>,
) -> Result<VerifiedManifestAttestation, OrchestratorError> {
    if repository != "tailrocks/velnor-new"
        || repository_id != 1_390_620_900
        || owner_id != 78_806_509
        || !is_sha(source_sha)
        || !is_sha256(subject.sha256)
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
        subject,
    )?;
    Ok(VerifiedManifestAttestation {
        source_sha: source_sha.to_owned(),
        subject_sha256: subject.sha256.to_owned(),
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
    subject: &SubjectExpectation<'_>,
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
        || !valid_subjects(
            &statement.subject,
            subject.name,
            subject.sha256,
            subject.binary,
        )
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

fn valid_subjects(subjects: &[Subject], name: &str, digest: &str, binary: bool) -> bool {
    let expected = if binary {
        vec![
            name.to_owned(),
            format!("{name}.sha256"),
            format!("{name}.provenance.json"),
        ]
    } else {
        vec![name.to_owned()]
    };
    subjects.len() == expected.len()
        && expected
            .iter()
            .all(|wanted| subjects.iter().filter(|item| &item.name == wanted).count() == 1)
        && subjects.iter().all(|item| {
            item.digest.len() == 1
                && item
                    .digest
                    .get("sha256")
                    .is_some_and(|value| is_sha256(value))
        })
        && subjects
            .iter()
            .find(|item| item.name == name)
            .and_then(|item| item.digest.get("sha256"))
            .is_some_and(|value| value == digest)
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

#[cfg(test)]
mod subject_tests {
    use super::*;
    const SOURCE: &str = "f1041f322c54cd7edfdef06afd307799eeaa801a";
    const MANIFEST_SHA: &str = "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52";
    const BINARY_SHA: &str = "040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3";
    const BINARY: &str = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
    const MANIFEST: &[u8] = include_bytes!("../tests/attestation_fixtures/manifest-verified.json");
    const EXECUTABLE: &[u8] = include_bytes!("../tests/attestation_fixtures/binary-verified.json");
    fn parse(
        raw: &[u8],
        name: &str,
        sha: &str,
        binary: bool,
    ) -> Result<VerifiedManifestAttestation, OrchestratorError> {
        parse_verified_release_attestation(
            raw,
            "tailrocks/velnor-new",
            1_390_620_900,
            78_806_509,
            SOURCE,
            &SubjectExpectation {
                name,
                sha256: sha,
                binary,
            },
        )
    }
    #[test]
    fn verified_subjects_are_bound_to_exact_release_and_run()
    -> Result<(), Box<dyn std::error::Error>> {
        let manifest = parse_verified_manifest_attestation(
            MANIFEST,
            "tailrocks/velnor-new",
            1_390_620_900,
            78_806_509,
            SOURCE,
            MANIFEST_SHA,
        )?;
        let executable = parse(EXECUTABLE, BINARY, BINARY_SHA, true)?;
        assert!(same_release_run(&manifest, &executable));
        let bytes = include_bytes!("../tests/attestation_fixtures/release-manifest.json");
        bound_release_manifest(bytes, &manifest)?;
        let mut altered = bytes.to_vec();
        altered.push(b' ');
        assert!(bound_release_manifest(&altered, &manifest).is_err());
        let mut changed_run = executable.clone();
        changed_run.run_id += 1;
        assert!(!same_release_run(&manifest, &changed_run));
        let other_run = std::str::from_utf8(EXECUTABLE)?.replace("37904531694", "37904531695");
        let other = parse(other_run.as_bytes(), BINARY, BINARY_SHA, true)?;
        assert!(!same_release_run(&manifest, &other));
        assert!(parse(EXECUTABLE, BINARY, MANIFEST_SHA, true).is_err());
        assert!(
            parse(
                EXECUTABLE,
                "velnor-actions-0.1.5-aarch64-apple-darwin",
                BINARY_SHA,
                true
            )
            .is_err()
        );
        assert!(parse(EXECUTABLE, BINARY, BINARY_SHA, false).is_err());
        assert!(parse(MANIFEST, BINARY, BINARY_SHA, true).is_err());
        let mut value: serde_json::Value = serde_json::from_slice(EXECUTABLE)?;
        let subjects = value[0]["verificationResult"]["statement"]["subject"]
            .as_array_mut()
            .ok_or("subjects")?;
        subjects.push(subjects[0].clone());
        assert!(parse(&serde_json::to_vec(&value)?, BINARY, BINARY_SHA, true).is_err());
        let mut missing: serde_json::Value = serde_json::from_slice(EXECUTABLE)?;
        missing[0]["verificationResult"]["statement"]["subject"]
            .as_array_mut()
            .ok_or("subjects")?
            .pop();
        assert!(parse(&serde_json::to_vec(&missing)?, BINARY, BINARY_SHA, true).is_err());
        let mut invalid: serde_json::Value = serde_json::from_slice(EXECUTABLE)?;
        invalid[0]["verificationResult"]["statement"]["subject"][1]["digest"]["sha256"] =
            serde_json::json!("not-a-sha");
        assert!(parse(&serde_json::to_vec(&invalid)?, BINARY, BINARY_SHA, true).is_err());
        Ok(())
    }

    #[test]
    fn rejects_wrong_attestation_identity() {
        assert!(
            parse_verified_release_attestation(
                EXECUTABLE,
                "tailrocks/other",
                1_390_620_900,
                78_806_509,
                SOURCE,
                &SubjectExpectation {
                    name: BINARY,
                    sha256: BINARY_SHA,
                    binary: true
                }
            )
            .is_err()
        );
        assert!(
            parse_verified_release_attestation(
                EXECUTABLE,
                "tailrocks/velnor-new",
                1_390_620_901,
                78_806_509,
                SOURCE,
                &SubjectExpectation {
                    name: BINARY,
                    sha256: BINARY_SHA,
                    binary: true
                }
            )
            .is_err()
        );
        assert!(
            parse_verified_release_attestation(
                EXECUTABLE,
                "tailrocks/velnor-new",
                1_390_620_900,
                78_806_509,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &SubjectExpectation {
                    name: BINARY,
                    sha256: BINARY_SHA,
                    binary: true
                }
            )
            .is_err()
        );
    }

    fn check_optional_public_tool_measurements(
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        receipt: &crate::crate_release_journal::JournalReceipt,
        generator: &AttestedLinuxGenerator,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(root) = std::env::var_os("VELNOR_PUBLIC_TOOL_FIXTURE") {
            check_public_tool_measurements(
                std::path::Path::new(&root),
                intent,
                receipt,
                generator,
            )?;
        }
        Ok(())
    }

    fn check_public_tool_measurements(
        source: &std::path::Path,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        receipt: &crate::crate_release_journal::JournalReceipt,
        generator: &AttestedLinuxGenerator,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let paths = [
            "velnor/bootstrap/velnor-actions-0.1.5-x86_64-unknown-linux-gnu",
            "velnor-release-plz/release-plz",
            "velnor/rust-manifest/channel.toml",
        ];
        for path in paths {
            let target = root.path().join(path);
            std::fs::create_dir_all(target.parent().ok_or("parent")?)?;
            std::fs::copy(source.join(path), target)?;
        }
        assert_eq!(
            measure_release_tools(root.path(), intent, receipt, generator)?.into_pins(),
            receipt.expected_tools
        );
        for path in paths {
            let target = root.path().join(path);
            std::fs::write(&target, b"tampered")?;
            assert!(measure_release_tools(root.path(), intent, receipt, generator).is_err());
            std::fs::copy(source.join(path), target)?;
        }
        Ok(())
    }

    #[test]
    fn expected_tool_pins_match_reviewed_intent_and_attested_generator()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::crate_release_journal::{
            JournalAuthentication, JournalIdentity, JournalReceipt, JournalRegistry,
            JournalToolPins,
        };
        let intent: velnor_actions_contract::config::CrateReleaseIntent = serde_json::from_value(
            serde_json::json!({"schema":1,"repository":"example/demo","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":"1.97.1","generator_version":"0.1.5","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":null}),
        )?;
        let mut generator = AttestedLinuxGenerator {
            version: "0.1.5".into(),
            sha256: BINARY_SHA.into(),
            source_sha: SOURCE.into(),
            run_id: 37_904_531_694,
            run_attempt: 1,
        };
        let release_plz =
            velnor_actions_mise::catalog::qualified_release_plz("0.4.3", "ubuntu-24.04")
                .ok_or("catalog")?;
        let rust =
            velnor_actions_mise::catalog::rust_channel_manifest_sha256("1.97.1").ok_or("rust")?;
        let mut receipt = JournalReceipt {
            semver_evidence: std::collections::BTreeMap::new(),
            predecessor: None,
            identity: JournalIdentity {
                repository: "example/demo".into(),
                workflow_sha: "a".repeat(40),
                source_sha: "b".repeat(40),
                run_id: 12,
                run_attempt: 1,
            },
            intent_sha256: "c".repeat(64),
            base_intent_sha256: "c".repeat(64),
            registry: JournalRegistry::CratesIo,
            generator_version: "0.1.5".into(),
            release_plz_version: "0.4.3".into(),
            rust_version: "1.97.1".into(),
            expected_tools: JournalToolPins {
                generator_sha256: BINARY_SHA.into(),
                release_plz_sha256: release_plz.executable_sha256.into(),
                rust_manifest_sha256: rust.into(),
            },
            authentication: JournalAuthentication::TrustedPublishing,
            required_check_id: 1,
            trusted_reviewer_id: 139_017,
            workflow_run_url: "https://github.com/example/demo/actions/runs/12".into(),
            source_files: vec![],
            packages: vec![],
        };
        let derived = derive_attested_tool_expectations(&intent, &generator)?;
        assert_eq!(derived, receipt.expected_tools);
        match_attested_tool_expectations(&intent, &receipt, &generator)?;
        check_optional_public_tool_measurements(&intent, &receipt, &generator)?;
        generator.source_sha = "d".repeat(40);
        assert!(derive_attested_tool_expectations(&intent, &generator).is_err());
        generator.source_sha = SOURCE.into();
        receipt.expected_tools.generator_sha256 = "0".repeat(64);
        assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
        receipt.expected_tools.generator_sha256 = BINARY_SHA.into();
        receipt.expected_tools.release_plz_sha256 = "0".repeat(64);
        assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
        receipt.expected_tools.release_plz_sha256 = release_plz.executable_sha256.into();
        receipt.expected_tools.rust_manifest_sha256 = "0".repeat(64);
        assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
        receipt.expected_tools.rust_manifest_sha256 = rust.into();
        let mut invalid = intent.clone();
        invalid.runner = "ubuntu-22.04".into();
        assert!(match_attested_tool_expectations(&invalid, &receipt, &generator).is_err());
        invalid = intent.clone();
        invalid.generator_version = "0.1.6".into();
        assert!(match_attested_tool_expectations(&invalid, &receipt, &generator).is_err());
        invalid = intent.clone();
        invalid.release_plz_version = "0.4.4".into();
        assert!(match_attested_tool_expectations(&invalid, &receipt, &generator).is_err());
        receipt.generator_version = "0.1.6".into();
        assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
        receipt.generator_version = "0.1.5".into();
        receipt.release_plz_version = "0.4.4".into();
        assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
        check_wrong_rust_version(&intent, &mut receipt, &generator);
        Ok(())
    }

    fn check_wrong_rust_version(
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        receipt: &mut crate::crate_release_journal::JournalReceipt,
        generator: &AttestedLinuxGenerator,
    ) {
        receipt.release_plz_version = "0.4.3".into();
        receipt.rust_version = "1.97.2".into();
        assert!(match_attested_tool_expectations(intent, receipt, generator).is_err());
    }

    #[test]
    #[ignore = "requires downloaded public release assets and GitHub authentication"]
    fn verifies_public_manifest_and_linux_binary() -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write as _;
        let fixture =
            std::env::var_os("VELNOR_ATTESTATION_TEST_DIR").ok_or("missing fixture directory")?;
        let root = std::path::Path::new(&fixture);
        let manifest_path = root.join("release-manifest.json");
        let binary_path = root.join(BINARY);
        let deadline = CheckDeadline::after(std::time::Duration::from_secs(180))?;
        let manifest = verify_published_manifest_attestation(
            &manifest_path,
            &root.join("release-manifest.json.intoto.jsonl"),
            &SdkManifestAttestationExpectation {
                repository: "tailrocks/velnor-new",
                repository_id: 1_390_620_900,
                owner_id: 78_806_509,
                source_sha: SOURCE,
                manifest_sha256: MANIFEST_SHA,
            },
            deadline,
        )?;
        let bytes = std::fs::read(&manifest_path)?;
        let verified = verify_published_linux_binary_attestation(
            &binary_path,
            &root.join(format!("{BINARY}.intoto.jsonl")),
            &bytes,
            &manifest,
            deadline,
        )?;
        assert_eq!(verified.version, "0.1.5");
        assert_eq!(verified.sha256, BINARY_SHA);
        assert_eq!(verified.source_sha, SOURCE);
        assert_eq!(verified.run_id, 37_904_531_694);
        assert_eq!(verified.run_attempt, 1);
        let mut forged = bytes;
        forged.push(b' ');
        assert!(
            verify_published_linux_binary_attestation(
                &binary_path,
                &root.join(format!("{BINARY}.intoto.jsonl")),
                &forged,
                &manifest,
                deadline,
            )
            .is_err()
        );
        let temp = tempfile::tempdir()?;
        let copy = temp.path().join(BINARY);
        std::fs::copy(&binary_path, &copy)?;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&copy)?
            .write_all(b"x")?;
        assert!(
            verify_published_linux_binary_attestation(
                &copy,
                &root.join(format!("{BINARY}.intoto.jsonl")),
                &std::fs::read(&manifest_path)?,
                &manifest,
                deadline,
            )
            .is_err()
        );
        let bad_bundle = temp.path().join("invalid.intoto.jsonl");
        std::fs::write(&bad_bundle, b"invalid bundle")?;
        assert!(
            verify_published_linux_binary_attestation(
                &binary_path,
                &bad_bundle,
                &std::fs::read(&manifest_path)?,
                &manifest,
                deadline,
            )
            .is_err()
        );
        Ok(())
    }
}
