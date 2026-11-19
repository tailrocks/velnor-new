// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{DoctorProbeStartResult, DoctorProbeState};
use crate::error::HostError;
use crate::journal::{
    CapacityLimits, Journal, OperationalCapacityLimits, ProbePhase, ProbeSeed,
    ResourceProbePurpose, capacity_request_sha256,
};

const NONCE: &str = "0123456789abcdef0123456789abcdef";
const OPERATION_ID: &str = "fedcba9876543210fedcba9876543210";
const ROOT_DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";

async fn uncertain_ticket(path: &std::path::Path) -> Result<Journal, HostError> {
    let journal = Journal::open(path).await?;
    journal.bind_engine("engine-test").await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 8_000,
                memory_bytes: 32 << 30,
            },
            OperationalCapacityLimits {
                cpu_millicores: 8_000,
                memory_bytes: 32 << 30,
            },
        )
        .await?;
    let DoctorProbeStartResult::Created(ticket) = journal
        .start_doctor_probe_request(NONCE, "engine-test", ROOT_DIGEST)
        .await?
    else {
        return Err(HostError::Journal);
    };
    let seed = ProbeSeed {
        operation_id: OPERATION_ID.to_owned(),
        instance_id: journal.instance_id().await?,
        engine_id: "engine-test".to_owned(),
        docker_root_digest: ROOT_DIGEST.to_owned(),
        source_revision: "2".repeat(40),
        runtime_image_id: format!("sha256:{}", "3".repeat(64)),
        image_binding_digest: "4".repeat(64),
        operation_name: format!("velnor-resource-probe-{OPERATION_ID}"),
        projection_digest: "5".repeat(64),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: capacity_request_sha256(),
    };
    journal
        .prepare_probe_for_doctor_request(&ticket, seed)
        .await?;
    journal
        .transition_probe(
            OPERATION_ID,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    journal
        .mark_doctor_probe_uncertain(&ticket, OPERATION_ID)
        .await?;
    Ok(journal)
}

async fn uncertain_ticket_with_aborted_child(path: &std::path::Path) -> Result<(), HostError> {
    uncertain_ticket(path)
        .await?
        .abort_unattached_probe(OPERATION_ID, "engine-test", ROOT_DIGEST)
        .await
}

#[tokio::test]
async fn aborted_child_of_an_uncertain_probe_is_recovered_as_stale_on_open() -> Result<(), HostError>
{
    let dir = tempfile::tempdir().map_err(|_| HostError::Path)?;
    let path = dir.path().join("journal.db");
    uncertain_ticket_with_aborted_child(&path).await?;
    let journal = Journal::open(&path).await?;
    journal.recover_after_restart_without_authority().await?;
    let record = journal
        .doctor_probe_request_by_nonce(NONCE)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(record.state(), DoctorProbeState::Stale);
    assert!(journal.leased_probe().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn uncertain_probe_with_a_quarantined_child_is_not_marked_stale() -> Result<(), HostError> {
    let dir = tempfile::tempdir().map_err(|_| HostError::Path)?;
    let path = dir.path().join("journal.db");
    drop(uncertain_ticket(&path).await?);
    let journal = Journal::open(&path).await?;
    journal.recover_after_restart_without_authority().await?;
    let record = journal
        .doctor_probe_request_by_nonce(NONCE)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(record.state(), DoctorProbeState::Recovering);
    Ok(())
}
