// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release, invoke_upload};
use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
use crate::crate_release_journal_adapter::JournalAdapter;

use super::journal_start::UploadRuntime;
use super::{
    Duration, JournalRuntime, NativeGitHubAdmissionRead, OrchestratorError, internal,
    read_manifest_at_workflow, read_receipt, required, start_journal_with, thread,
    verify_bootstrap_attestation,
};

pub(super) fn coordinate_journal(runtime: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let execution = match runtime.upload {
        UploadRuntime::Unprivileged(execution) => execution,
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        UploadRuntime::Supervised(_) => {
            return Err(internal("crate_release_completion_requires_close"));
        }
    };
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let UploadRuntime::Unprivileged(execution) = runtime.upload;
    let frozen = prepare_coordinate_journal(runtime)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(runtime.token.to_owned())?);
    frozen.record_unprivileged(execution, &journal, runtime.github, runtime.registry)?;
    Ok(())
}

pub(super) fn prepare_coordinate_journal(
    runtime: JournalRuntime<'_>,
) -> Result<crate::crate_release_controller::FrozenCompletion, OrchestratorError> {
    let source_execution = runtime.upload.source_execution();
    let identity = JournalIdentity {
        repository: runtime.request.repository.clone(),
        workflow_sha: runtime.request.workflow_sha.clone(),
        source_sha: runtime.request.source_sha.clone(),
        run_id: runtime.request.run_id,
        run_attempt: runtime.request.run_attempt,
    };
    let admission = read_receipt(runtime.receipt_path)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(runtime.token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        source_execution,
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent: runtime.intent,
        request: runtime.request,
        github: runtime.github,
        registry: runtime.registry,
        source: runtime.source,
        git: runtime.git,
        sequence: 1,
    };
    let runner_temp = runtime.runner_temp;
    let workflow_root = runtime.workflow_root;
    coordinate_operations(runtime, &input)?;
    let reviewed_manifest =
        read_manifest_at_workflow(workflow_root, &runtime.request.workflow_sha, runtime.git)?;
    let mut completion = None;
    start_journal_with(
        &runner_temp.join("velnor/bootstrap"),
        runtime.intent,
        &reviewed_manifest,
        verify_bootstrap_attestation,
        |generator| {
            let tools = crate::release::package_release_signer::measure_release_tools(
                runner_temp,
                runtime.intent,
                &started.receipt,
                &generator,
            )?;
            completion = Some(match runtime.upload {
                UploadRuntime::Unprivileged(execution) => {
                    crate::crate_release_controller::FrozenCompletion::prepare_unprivileged(
                        &input, tools, execution,
                    )?
                }
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                UploadRuntime::Supervised(execution) => {
                    crate::crate_release_controller::FrozenCompletion::prepare_supervised(
                        &input, tools, execution,
                    )?
                }
            });
            Ok(())
        },
    )?;
    completion.ok_or_else(|| internal("crate_release_completion_missing"))
}

fn coordinate_operations<
    J: crate::crate_release_journal::JournalIo + crate::crate_release_journal::JournalOperationRefs,
>(
    runtime: JournalRuntime<'_>,
    input: &CrateReleaseObservation<
        '_,
        J,
        NativeGitHubAdmissionRead,
        super::NativeCrateRegistryRead,
    >,
) -> Result<(), OrchestratorError> {
    let runner_temp = runtime.runner_temp;
    let credential = match runtime.upload {
        UploadRuntime::Unprivileged(_) => {
            Some(upload_credentials(runtime.intent.authentication, required))
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        UploadRuntime::Supervised(_) => None,
    };
    coordinate_release(
        input,
        |package| {
            match runtime.upload {
                UploadRuntime::Unprivileged(_) => {
                    credential
                        .as_ref()
                        .ok_or_else(|| internal("crate_release_upload_credential_missing"))?
                        .as_ref()
                        .map_err(|_| internal("crate_release_upload_credential_missing"))?;
                }
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                UploadRuntime::Supervised(execution) => {
                    execution.credential_preflight(runtime.source, runtime.intent, runtime.git)?;
                }
            }
            crate::release::crate_release_semver::verify_before_upload(
                runtime.source,
                input.admission,
                runtime.registry,
                runner_temp,
                package,
                runtime.git,
                input.source_execution,
            )
        },
        |permit| match runtime.upload {
            UploadRuntime::Unprivileged(_) => {
                let credential = credential
                    .as_ref()
                    .ok_or_else(|| internal("crate_release_upload_credential_missing"))?
                    .as_ref()
                    .map_err(|_| internal("crate_release_upload_credential_missing"))?;
                invoke_upload(
                    input,
                    permit,
                    runner_temp,
                    runtime.token,
                    credential.as_publish_credential(),
                )
            }
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            UploadRuntime::Supervised(execution) => {
                crate::crate_release_controller::invoke_supervised_upload(input, permit, execution)
            }
        },
        || thread::sleep(coordinator_wait_duration()),
    )?;
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(super) fn complete_journal(
    completion: crate::release::crate_release_source_view::ClosedCompletion,
    token: &str,
    github: &NativeGitHubAdmissionRead,
    registry: &super::NativeCrateRegistryRead,
) -> Result<(), OrchestratorError> {
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(token.to_owned())?);
    completion.complete(&journal, github, registry)?;
    Ok(())
}

pub(super) fn coordinator_wait_duration() -> Duration {
    #[cfg(test)]
    if std::env::var_os("VELNOR_TEST_FAST_POLL").is_some() {
        return Duration::ZERO;
    }
    Duration::from_secs(2)
}

enum UploadCredential {
    Trusted {
        request_token: String,
        request_url: String,
    },
    Bootstrap {
        cargo_token: String,
    },
}

impl UploadCredential {
    fn as_publish_credential(&self) -> velnor_actions_mise::command::CratePublishCredential<'_> {
        use velnor_actions_mise::command::CratePublishCredential;
        match self {
            Self::Trusted {
                request_url,
                request_token,
            } => CratePublishCredential::Trusted {
                request_url,
                request_token,
            },
            Self::Bootstrap { cargo_token } => CratePublishCredential::Bootstrap { cargo_token },
        }
    }
}

fn upload_credentials(
    authentication: velnor_actions_contract::config::ReleaseAuthentication,
    mut read: impl FnMut(&str) -> Result<String, OrchestratorError>,
) -> Result<UploadCredential, OrchestratorError> {
    use velnor_actions_contract::config::ReleaseAuthentication;
    match authentication {
        ReleaseAuthentication::TrustedPublishing => Ok(UploadCredential::Trusted {
            request_token: read("ACTIONS_ID_TOKEN_REQUEST_TOKEN")?,
            request_url: read("ACTIONS_ID_TOKEN_REQUEST_URL")?,
        }),
        ReleaseAuthentication::BootstrapToken => Ok(UploadCredential::Bootstrap {
            cargo_token: read("CARGO_REGISTRY_TOKEN")?,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{UploadCredential, upload_credentials};
    use crate::internal::internal;
    use std::collections::BTreeMap;
    use velnor_actions_contract::config::ReleaseAuthentication;

    #[test]
    fn credential_capture_preserves_selected_mode_and_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut input = BTreeMap::from([
            ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "oidc-fixture"),
            (
                "ACTIONS_ID_TOKEN_REQUEST_URL",
                "https://fixture.invalid/request",
            ),
            ("CARGO_REGISTRY_TOKEN", "bootstrap-fixture"),
        ]);
        let read = |name: &str| {
            input
                .get(name)
                .filter(|value| !value.is_empty())
                .map(|value| (*value).to_owned())
                .ok_or_else(|| internal("missing_fixture_credential"))
        };
        let trusted = upload_credentials(ReleaseAuthentication::TrustedPublishing, read)?;
        let bootstrap = upload_credentials(ReleaseAuthentication::BootstrapToken, read)?;
        input.insert("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "changed-fixture");
        input.insert("CARGO_REGISTRY_TOKEN", "changed-fixture");
        assert!(
            matches!(trusted, UploadCredential::Trusted { request_token, request_url }
            if request_token == "oidc-fixture" && request_url == "https://fixture.invalid/request")
        );
        assert!(
            matches!(bootstrap, UploadCredential::Bootstrap { cargo_token }
            if cargo_token == "bootstrap-fixture")
        );
        Ok(())
    }

    #[test]
    fn publication_never_falls_back_to_another_authentication_mode() {
        let read = |name: &str| match name {
            "CARGO_REGISTRY_TOKEN" => Ok("bootstrap-fixture".to_owned()),
            _ => Err(internal("missing_fixture_credential")),
        };
        assert!(upload_credentials(ReleaseAuthentication::TrustedPublishing, read).is_err());
        let read = |name: &str| match name {
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN" => Ok("oidc-fixture".to_owned()),
            "ACTIONS_ID_TOKEN_REQUEST_URL" => Ok("https://fixture.invalid/request".to_owned()),
            _ => Err(internal("missing_fixture_credential")),
        };
        assert!(upload_credentials(ReleaseAuthentication::BootstrapToken, read).is_err());
    }
}
