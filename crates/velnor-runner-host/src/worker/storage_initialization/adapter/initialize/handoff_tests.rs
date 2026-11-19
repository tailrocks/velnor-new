// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::time::Instant;

use crate::{
    config::RunnerPlatform,
    journal::{Journal, StorageRole},
    launch::bootstrap::unbound_pair_fixtures::{
        FAILED,
        fixture::FixtureDocker,
        seed::{self, Pair},
    },
    worker::ResourceBudgetConfig,
};

use super::*;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const INITIALIZER: &str = "SELECT create_state || '/' || start_state || '/' || result_state || '/' || remove_state || '/' || abort_state FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";
const STATES: &str = "SELECT r.state || '/' || i.state FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id WHERE r.id = ?1";

async fn failed_handoff(
    journal: &Journal,
    docker: &Docker,
    scope: &ConnectionJournal,
    fixture: &FixtureDocker,
    pair: Pair,
) -> TestResult<StorageInitializationClaim> {
    seed::initializer(journal, pair, "not_requested", "not_requested").await?;
    seed::unguarded(
        journal,
        &[
            format!(
                "DELETE FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {}",
                pair.reservation
            ),
            format!(
                "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'not_requested', runner_container_id = NULL, dind_container_id = NULL WHERE reservation_id = {}",
                pair.reservation
            ),
            format!(
            "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received', start_state = 'started', result_state = 'confirmed', home_device_major = 0, home_device_minor = 1, home_inode = 1, home_mount_id = 1, work_device_major = 0, work_device_minor = 2, work_inode = 2, work_mount_id = 2, metadata_layout_digest = '{}' WHERE reservation_id = {}",
            "a".repeat(64),
            pair.reservation
        ),
        ],
    )
    .await?;
    fixture.add_initializer(pair);
    let _ = docker;
    scope
        .storage_initializer_abort_candidate(pair.reservation)
        .await?
        .ok_or_else(|| "the confirmed initializer has no claim".into())
}

#[tokio::test]
async fn a_failed_handoff_records_uncertainty_and_the_next_recovery_removes_only_the_journaled_helper()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    let claim = failed_handoff(&journal, &docker, &scope, &fixture, FAILED).await?;
    let image = StorageInitializationImage::for_test(
        &"1".repeat(64),
        &"1".repeat(40),
        RunnerPlatform::LinuxAmd64,
    );
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let limits = StorageInitializationLimits::from_reserved_budget(budget, 500, 64 << 20, 16)?;
    let metadata = VerifiedCreatedStorageObject::for_test(
        FAILED.reservation,
        FAILED.reservation - 1,
        engine.engine_id(),
        engine.docker_root_digest(),
        StorageRole::EngineMetadata,
        &format!("w{}-metadata", FAILED.nonce()),
    );
    let context = InitializationContext {
        docker: &docker,
        journal: &scope,
        reservation_id: FAILED.reservation,
        image: &image,
        limits,
        metadata: &metadata,
        deadline: Instant::now() + Duration::from_secs(30),
        phase: InitializationPhase::Handoff,
        active_claim: Some(claim),
        claim_pending: false,
        response_evidence: None,
    };
    assert_eq!(
        seed::text(&journal, STATES, FAILED.reservation).await?,
        "reserved/pending"
    );

    context.resolve_failure().await?;
    context.resolve_failure().await?;

    assert_eq!(
        seed::text(&journal, STATES, FAILED.reservation).await?,
        "uncertain/uncertain"
    );
    assert!(fixture.has_initializer(FAILED));
    recover_absent(
        &docker,
        &scope,
        FAILED.reservation,
        Instant::now() + Duration::from_secs(30),
    )
    .await?;
    assert!(!fixture.has_initializer(FAILED));
    assert_eq!(
        seed::text(&journal, INITIALIZER, FAILED.reservation).await?,
        "created/started/confirmed/not_requested/absent"
    );
    Ok(())
}
