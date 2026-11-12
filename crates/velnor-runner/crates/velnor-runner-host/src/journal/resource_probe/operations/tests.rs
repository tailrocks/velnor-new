// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ProbePhase, ProbeSeed, ResourceProbePurpose, capacity_request_sha256};
use crate::error::HostError;
use crate::journal::{CapacityLimits, Journal, OperationalCapacityLimits};

const OPERATION_ID: &str = "0123456789abcdef0123456789abcdef";
const CONTAINER_ID: &str = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd";

async fn journal_with_probe(dir: &tempfile::TempDir) -> Result<Journal, HostError> {
    let journal = Journal::open(&dir.path().join("journal.db")).await?;
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
    let seed = ProbeSeed {
        operation_id: OPERATION_ID.to_owned(),
        instance_id: journal.instance_id().await?,
        engine_id: "engine-test".to_owned(),
        docker_root_digest: "1".repeat(64),
        source_revision: "2".repeat(40),
        runtime_image_id: format!("sha256:{}", "3".repeat(64)),
        image_binding_digest: "4".repeat(64),
        operation_name: format!("velnor-resource-probe-{OPERATION_ID}"),
        projection_digest: "5".repeat(64),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: capacity_request_sha256(),
    };
    journal.prepare_probe(seed).await?;
    Ok(journal)
}

async fn create_requested(journal: &Journal) -> Result<(), HostError> {
    journal
        .transition_probe(
            OPERATION_ID,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await
}

#[tokio::test]
async fn unattached_quarantined_probe_binds_its_container() -> Result<(), HostError> {
    let dir = tempfile::tempdir().map_err(|_| HostError::Path)?;
    let journal = journal_with_probe(&dir).await?;
    create_requested(&journal).await?;
    journal.quarantine_probe(OPERATION_ID).await?;
    journal
        .bind_probe_container(OPERATION_ID, CONTAINER_ID)
        .await?;
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::ContainerCreated);
    assert_eq!(row.container_id.as_deref(), Some(CONTAINER_ID));
    Ok(())
}

#[tokio::test]
async fn quarantined_probe_with_a_container_is_not_rebound() -> Result<(), HostError> {
    let dir = tempfile::tempdir().map_err(|_| HostError::Path)?;
    let journal = journal_with_probe(&dir).await?;
    create_requested(&journal).await?;
    journal
        .bind_probe_container(OPERATION_ID, CONTAINER_ID)
        .await?;
    journal.quarantine_probe(OPERATION_ID).await?;
    let other = "f".repeat(64);
    assert!(matches!(
        journal.bind_probe_container(OPERATION_ID, &other).await,
        Err(HostError::Journal)
    ));
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::Quarantined);
    assert_eq!(row.container_id.as_deref(), Some(CONTAINER_ID));
    Ok(())
}

#[tokio::test]
async fn prepared_probe_without_a_container_is_not_bound() -> Result<(), HostError> {
    let dir = tempfile::tempdir().map_err(|_| HostError::Path)?;
    let journal = journal_with_probe(&dir).await?;
    assert!(matches!(
        journal
            .bind_probe_container(OPERATION_ID, CONTAINER_ID)
            .await,
        Err(HostError::Journal)
    ));
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::Prepared);
    assert_eq!(row.container_id, None);
    Ok(())
}

const ROOT_DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[tokio::test]
async fn aborting_an_unattached_probe_releases_its_lease_in_one_step() -> Result<(), HostError> {
    let dir = tempfile::tempdir().map_err(|_| HostError::Path)?;
    let journal = journal_with_probe(&dir).await?;
    create_requested(&journal).await?;
    journal
        .abort_unattached_probe(OPERATION_ID, "engine-test", ROOT_DIGEST)
        .await?;
    assert!(journal.leased_probe().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn aborting_a_probe_with_a_container_or_another_engine_changes_nothing()
-> Result<(), HostError> {
    let dir = tempfile::tempdir().map_err(|_| HostError::Path)?;
    let journal = journal_with_probe(&dir).await?;
    create_requested(&journal).await?;
    assert!(matches!(
        journal
            .abort_unattached_probe(OPERATION_ID, "another-engine", ROOT_DIGEST)
            .await,
        Err(HostError::Journal)
    ));
    journal
        .bind_probe_container(OPERATION_ID, CONTAINER_ID)
        .await?;
    assert!(matches!(
        journal
            .abort_unattached_probe(OPERATION_ID, "engine-test", ROOT_DIGEST)
            .await,
        Err(HostError::Journal)
    ));
    let row = journal.leased_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(row.phase, ProbePhase::ContainerCreated);
    Ok(())
}
