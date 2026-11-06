// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{AttestedLinuxGenerator, OrchestratorError, internal};

pub(crate) struct AdmittedStart<'a, J, G, R> {
    pub(crate) journal: &'a J,
    pub(crate) github: &'a G,
    pub(crate) registry: &'a R,
    pub(crate) intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    pub(crate) request: &'a crate::crate_release_github::GitHubAdmissionRequest,
    pub(crate) admission: &'a crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    pub(crate) source: &'a std::path::Path,
    pub(crate) git: &'a crate::crate_release_git::ReleaseGit,
    pub(crate) source_execution: &'a dyn crate::CrateSourceExecution,
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
    let observed = crate::crate_release_controller::reprepare_crate_release(input.admission,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
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
        input.git,
    )?;
    if verified_prior_chain(input, &receipt)? != prior_history {
        return Err(internal("crate_release_previous_changed"));
    }
    if crate::crate_release_controller::reprepare_crate_release(input.admission,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
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
    let chain = crate::crate_release_journal::inspect_prior_chain_with(
        input.journal,
        receipt,
        input.source,
        input.git,
        Some(&crate::crate_release_journal::PriorTransition {
            source_execution: input.source_execution, request: input.request, github: input.github,
        }),
    )?;
    for prior in &chain {
        let previous = &prior.start.receipt;
        let intent = match crate::crate_release_runtime::read_intent_at_workflow(
            input.workflow_root,
            &previous.identity.workflow_sha,
            input.git,
        ) {
            Ok(intent) => intent,
            Err(_) => input.source_execution.historical_intent_reader()?
                .verify_prior(previous, input.request, input.github)?,
        };
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
        JournalAuthentication, JournalIdentity, JournalReceipt, JournalRegistry,
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
    let packages = journal_packages(observed)?;
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
        source_files: crate::journal_source_inventory(
            input.source,
            &input.request.source_sha,
            input.git,
        )?,
        packages,
        semver_evidence: input.admission.semver_evidence.clone(),
    })
}

fn journal_packages(
    observed: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<Vec<crate::crate_release_journal::JournalPackage>, OrchestratorError> {
    use crate::crate_release_journal::{JournalOwner, JournalOwnerKind, JournalPackage};
    observed
        .packages
        .iter()
        .map(|package| {
            let owners = match &package.owners {
                crate::crate_release_registry::CrateRegistryOwnership::NewCrate => {
                    crate::crate_release_journal::JournalOwnership::NewCrate
                }
                crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } => {
                    let owners = owners
                        .iter()
                        .map(|owner| {
                            let kind = match owner.kind.as_str() {
                                "user" => JournalOwnerKind::User,
                                "team" => JournalOwnerKind::Team,
                                _ => return Err(internal("crate_release_start_owner_invalid")),
                            };
                            Ok(JournalOwner {
                                id: owner.id,
                                login: owner.login.clone(),
                                kind,
                            })
                        })
                        .collect::<Result<Vec<_>, OrchestratorError>>()?;
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
        .collect::<Result<Vec<_>, OrchestratorError>>()
}

pub(super) fn match_attested_tool_expectations(
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

pub(super) fn derive_attested_tool_expectations(
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
    if intent.runner != "ubuntu-26.04"
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
