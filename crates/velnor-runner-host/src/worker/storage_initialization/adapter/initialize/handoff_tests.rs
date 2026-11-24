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
const HANDOFFS: &str = "SELECT CAST(COUNT(*) AS TEXT) FROM capacity_reservation_storage_handoffs WHERE reservation_id = ?1";
const FAIL_HANDOFF: &str = "CREATE TRIGGER fail_handoff BEFORE INSERT ON capacity_reservation_storage_handoffs BEGIN SELECT RAISE(ABORT, 'handoff'); END";

async fn claimable(journal: &Journal, pair: Pair) -> TestResult<()> {
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
        ],
    )
    .await
}

#[tokio::test]
async fn a_failed_handoff_records_uncertainty_and_the_next_recovery_removes_only_the_journaled_helper()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    fixture.script_initializer(FAILED);
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    claimable(&journal, FAILED).await?;
    seed::execute(&journal, FAIL_HANDOFF).await?;
    let image = StorageInitializationImage::for_test(
        &format!("sha256:{}", "1".repeat(64)),
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
    assert_eq!(
        seed::text(&journal, STATES, FAILED.reservation).await?,
        "reserved/pending"
    );

    let outcome = initialize(
        &docker,
        &scope,
        FAILED.reservation,
        &image,
        limits,
        Duration::from_secs(30),
        &metadata,
    )
    .await;

    assert!(outcome.is_err());
    assert_eq!(
        seed::text(&journal, HANDOFFS, FAILED.reservation).await?,
        "0"
    );
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
