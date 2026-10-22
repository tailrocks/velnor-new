//! Check that readiness IPC uses controller-owned evidence.

use std::fs;

use crate::{
    config_test_support::{basic_connection, host_config},
    controller_ipc::wire::Request,
    journal::Journal,
};

use super::{ControlState, handle_request, test_state};
use crate::controller_ipc::ControllerReadinessProof;

#[tokio::test]
async fn readiness_ipc_stays_reconciling_without_controller_evidence() -> Result<(), String> {
    let state_path = test_state("readiness-without-controller-evidence");
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("The journal could not open. Error: {error}."))?;
    let response = handle_request(
        &journal,
        &ControlState::empty(),
        &state_path,
        Request::ControllerReadiness {
            config_sha256: "b".repeat(64),
            engine_id: "engine".to_owned(),
            docker_root_digest: "a".repeat(64),
        },
        "engine",
        &"a".repeat(64),
    )
    .await
    .map_err(|error| format!("The readiness request failed. Error: {error}."))?;
    if response.readiness != Some(crate::readiness::Readiness::Reconciling)
        || response.value.is_some()
        || response.doctor_probe.is_some()
    {
        return Err(
            "The controller must remain in the reconciling state without readiness evidence."
                .to_owned(),
        );
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())
}

#[tokio::test]
async fn readiness_ipc_accepts_a_recent_matching_controller_proof() -> Result<(), String> {
    let state_path = test_state("readiness-with-current-controller-proof");
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("The journal could not open. Error: {error}."))?;
    journal
        .bind_engine("readiness-engine")
        .await
        .map_err(|error| format!("The journal could not bind its engine. Error: {error}."))?;
    let (state, config_sha256) = publish_ready_test_proof(&state_path, &journal).await?;
    let response = handle_request(
        &journal,
        &state,
        &state_path,
        Request::ControllerReadiness {
            config_sha256,
            engine_id: "readiness-engine".to_owned(),
            docker_root_digest: "a".repeat(64),
        },
        "readiness-engine",
        &"a".repeat(64),
    )
    .await
    .map_err(|error| format!("The readiness request failed. Error: {error}."))?;
    if response.readiness != Some(crate::readiness::Readiness::Ready) {
        return Err("The controller rejected a current matching proof.".to_owned());
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())
}

async fn publish_ready_test_proof(
    state_path: &std::path::Path,
    journal: &Journal,
) -> Result<(ControlState, String), String> {
    let connection = basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    );
    let config_text = host_config(&[connection], 1);
    fs::write(state_path.join("host.toml"), &config_text).map_err(|error| error.to_string())?;
    let (config, config_sha256) = crate::readiness_scan::controller_config(state_path)
        .ok_or_else(|| "The test configuration did not parse.".to_owned())?;
    let binding = config
        .connections()
        .first()
        .ok_or_else(|| "The test configuration has no connection.".to_owned())?;
    let state = ControlState::new(&config);
    state.set_admission_ready(true);
    state
        .record_session_poll(binding, 17, 23, "session-1")
        .map_err(|error| format!("The controller could not record its poll. Error: {error}."))?;
    let polls = state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The controller did not retain its poll receipt.".to_owned())?;
    let now = std::time::Instant::now();
    let instance_id = journal
        .instance_id()
        .await
        .map_err(|error| error.to_string())?;
    let revision = journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    let full_capacity = crate::journal::ReadinessCapacityUsage {
        jobs: 2,
        cpu_millicores: 2_000,
        memory_bytes: 2_048,
        max_jobs: 2,
        max_cpu_millicores: 2_000,
        max_memory_bytes: 2_048,
        operational_cpu_millicores: 500,
        operational_memory_bytes: 512,
        operational_lease_quiescent: true,
    };
    if !full_capacity.valid_for_guest(2_500, 2_560, 512)
        || full_capacity.valid_for_guest(2_499, 2_560, 512)
        || full_capacity.valid_for_guest(2_500, 2_559, 512)
        || full_capacity.valid_for_guest(2_500, 2_560, 511)
    {
        return Err(
            "The capacity validator did not bind worker limits to the measured guest.".to_owned(),
        );
    }
    state
        .publish_readiness(ControllerReadinessProof {
            config_sha256: config_sha256.clone(),
            engine_id: "readiness-engine".to_owned(),
            docker_root_digest: "a".repeat(64),
            journal_instance_id: instance_id,
            journal_revision: revision,
            sample_generation: "b".repeat(32),
            sample_observed_at: now,
            collection_started_at: now,
            collection_finished_at: now,
            capacity: full_capacity,
            sample_cpu_millicores: 2_500,
            sample_memory_total_bytes: 3_000,
            sample_memory_available_bytes: 512,
            inventory_sha256: "c".repeat(64),
            poll_receipts: polls,
        })
        .map_err(|error| format!("The controller could not publish its proof. Error: {error}."))?;
    Ok((state, config_sha256))
}
