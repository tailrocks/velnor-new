// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::args::ImageMaintenanceCommand;
use velnor_actions_orchestrator::{CheckDeadline, OrchestratorError};

use super::mac_step_output;

pub(super) fn admit_publication_request(command: &ImageMaintenanceCommand, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let ImageMaintenanceCommand::AdmitMacPublication { source_commit, release_run_id, release_run_attempt, assets_dir, runner_temp } = command else {
        return Err(OrchestratorError::Internal { problem: "mac_publication_command".to_owned() });
    };
    let result = velnor_actions_orchestrator::admit_mac_publication(&velnor_actions_orchestrator::AdmitMacPublicationRequest {
        source_commit: source_commit.clone(), release_run_id: *release_run_id, release_run_attempt: *release_run_attempt,
        assets_dir: assets_dir.clone(), runner_temp: runner_temp.clone(), deadline,
    })?;
    println!("{}", serde_json::to_string(&result).map_err(|_| OrchestratorError::Internal { problem: "mac_publication_output".to_owned() })?);
    Ok(())
}

pub(super) fn verify_mac_request(
    command: &ImageMaintenanceCommand,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let ImageMaintenanceCommand::VerifyMacAttestation {
        producer_run_id,
        producer_run_attempt,
        producer_artifact_id,
        publisher_artifact_id,
        assets_dir,
        runner_temp,
    } = command
    else {
        return Err(OrchestratorError::Internal {
            problem: "mac_verification_command".to_owned(),
        });
    };
    let verified = velnor_actions_orchestrator::verify_mac_attestation(
        &velnor_actions_orchestrator::VerifyMacAttestationRequest {
            producer_run_id: *producer_run_id,
            producer_run_attempt: *producer_run_attempt,
            producer_artifact_id: *producer_artifact_id,
            publisher_artifact_id: *publisher_artifact_id,
            assets_dir: assets_dir.clone(),
            runner_temp: runner_temp.clone(),
            deadline,
        },
    )?;
    mac_step_output::write(&verified)
}

#[cfg(target_os = "macos")]
pub(super) fn qualify_mac_request(
    command: &ImageMaintenanceCommand,
) -> Result<(), OrchestratorError> {
    let ImageMaintenanceCommand::QualifyRunnerFamilyMac {
        manifest,
        runner_lock,
        source_commit,
        run_id,
        run_attempt,
        publisher_artifact_id,
        docker_socket,
        state_dir,
        receipt_output,
    } = command
    else {
        return Err(OrchestratorError::Internal {
            problem: "mac_qualification_command".to_owned(),
        });
    };
    let request = velnor_runner_host::resource_probe_release::MacRunnerQualificationRequest {
        manifest: manifest.clone(),
        runner_lock: runner_lock.clone(),
        source_commit: source_commit.clone(),
        run_id: *run_id,
        run_attempt: *run_attempt,
        publisher_artifact_id: *publisher_artifact_id,
        docker_socket: docker_socket.clone(),
        state_dir: state_dir.clone(),
        receipt_output: receipt_output.clone(),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| OrchestratorError::Internal {
            problem: "mac_qualification_runtime".to_owned(),
        })?;
    let qualifier = velnor_runner_host::resource_probe_release::MacRunnerQualifier::anonymous()
        .map_err(|error| OrchestratorError::Internal {
            problem: format!("mac_qualification_{}", error.code()),
        })?;
    runtime
        .block_on(qualifier.qualify(request))
        .map_err(|error| OrchestratorError::Internal {
            problem: format!("mac_qualification_{}", error.code()),
        })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn qualify_mac_request(_: &ImageMaintenanceCommand) -> Result<(), OrchestratorError> {
    Err(OrchestratorError::Internal {
        problem: "mac_qualification_unsupported_platform".to_owned(),
    })
}
