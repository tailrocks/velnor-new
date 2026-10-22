use super::{ControlState, handle_request, test_state};
use crate::controller_ipc::wire::Request;
use crate::journal::{
    CapacityLimits, DoctorProbeOutcome, DoctorProbeStartResult, DoctorProbeTicket, Journal,
    OperationalCapacityLimits, ProbePhase, ProbeSeed, ResourceProbePurpose,
    capacity_request_sha256,
};
use crate::launch::VerifiedProbeCleanup;
use std::fs;
use std::path::PathBuf;

struct CompletedProbeFixture {
    state_path: PathBuf,
    journal: Journal,
    state: ControlState,
    nonce: String,
}

#[tokio::test]
async fn completed_doctor_report_keeps_controller_unready() -> Result<(), String> {
    let fixture = completed_probe_fixture().await?;
    let response = handle_request(
        &fixture.journal,
        &fixture.state,
        &fixture.state_path,
        Request::DoctorProbeStatus {
            nonce: fixture.nonce,
        },
        "doctor-engine",
        &"a".repeat(64),
    )
    .await
    .map_err(|error| format!("status request: {error}"))?;
    let readiness = response.readiness;
    let reply = response
        .doctor_probe
        .ok_or_else(|| "status response omitted doctor data".to_owned())?;
    if reply.state != "completed"
        || reply.report != Some(report_facts())
        || readiness != Some(crate::readiness::Readiness::Reconciling)
    {
        return Err("completed probe facts incorrectly upgraded controller readiness".to_owned());
    }
    fs::remove_dir_all(fixture.state_path).map_err(|error| error.to_string())
}

async fn completed_probe_fixture() -> Result<CompletedProbeFixture, String> {
    let state_path = test_state("completed-doctor-probe-unready");
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;
    journal
        .bind_engine("doctor-engine")
        .await
        .map_err(|error| format!("bind engine: {error}"))?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 4 * 1024 * 1024 * 1024,
            },
            OperationalCapacityLimits {
                cpu_millicores: 100,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await
        .map_err(|error| format!("configure capacity: {error}"))?;

    let nonce = "2".repeat(32);
    let DoctorProbeStartResult::Created(ticket) = journal
        .start_doctor_probe_request(&nonce, "doctor-engine", &"a".repeat(64))
        .await
        .map_err(|error| format!("create request: {error}"))?
    else {
        return Err("the test did not create a doctor request".to_owned());
    };
    complete_child_probe(&journal, &ticket).await?;

    let state = ControlState::empty();
    state
        .save_doctor_report(&nonce, report_facts())
        .map_err(|error| format!("save doctor report: {error}"))?;
    Ok(CompletedProbeFixture {
        state_path,
        journal,
        state,
        nonce,
    })
}

async fn complete_child_probe(journal: &Journal, ticket: &DoctorProbeTicket) -> Result<(), String> {
    let operation_id = "b".repeat(32);
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    prepare_child_probe(journal, ticket, &operation_id, &operation_name).await?;
    remove_child_probe(journal, ticket, &operation_id, &operation_name).await
}

async fn prepare_child_probe(
    journal: &Journal,
    ticket: &DoctorProbeTicket,
    operation_id: &str,
    operation_name: &str,
) -> Result<(), String> {
    let child = ProbeSeed {
        operation_id: operation_id.to_owned(),
        instance_id: journal
            .instance_id()
            .await
            .map_err(|error| format!("read journal instance: {error}"))?,
        engine_id: "doctor-engine".to_owned(),
        docker_root_digest: "a".repeat(64),
        source_revision: "c".repeat(40),
        runtime_image_id: format!("sha256:{}", "d".repeat(64)),
        image_binding_digest: "e".repeat(64),
        operation_name: operation_name.to_owned(),
        projection_digest: "f".repeat(64),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: capacity_request_sha256(),
    };
    journal
        .prepare_probe_for_doctor_request(ticket, child)
        .await
        .map_err(|error| format!("prepare child: {error}"))?;
    journal
        .transition_probe(
            operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await
        .map_err(|error| format!("request child create: {error}"))
}

async fn remove_child_probe(
    journal: &Journal,
    ticket: &DoctorProbeTicket,
    operation_id: &str,
    operation_name: &str,
) -> Result<(), String> {
    let container_id = "7".repeat(64);
    journal
        .bind_probe_container(operation_id, &container_id)
        .await
        .map_err(|error| format!("bind child container: {error}"))?;
    journal
        .transition_probe(
            operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
        .await
        .map_err(|error| format!("request child removal: {error}"))?;
    journal
        .retire_probe(VerifiedProbeCleanup::test_only(
            operation_id.to_owned(),
            "doctor-engine".to_owned(),
            "a".repeat(64),
            ProbePhase::RemoveRequested,
            operation_name.to_owned(),
            Some(container_id),
        ))
        .await
        .map_err(|error| format!("retire child: {error}"))?;
    journal
        .finish_doctor_probe_request(ticket, operation_id, DoctorProbeOutcome::Completed)
        .await
        .map_err(|error| format!("complete request: {error}"))
}

fn report_facts() -> serde_json::Value {
    serde_json::json!({
        "cpu_count": 8,
        "docker_root_fs_type_name": "btrfs",
        "docker_root_inode_total": 0,
    })
}
