// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{ConnectionJournal, Journal, UnboundInitializer};

use super::fixture::FixtureDocker;
use super::seed::{self, Pair};
use super::{BOUND, FAILED, TestResult};

const STATES: &str = "SELECT remove_state || '/' || abort_state FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";

async fn seeded(
    dir: &std::path::Path,
) -> TestResult<(
    Journal,
    ConnectionJournal,
    crate::launch::SelectedEngineContext,
)> {
    let socket = dir.join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED, BOUND])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED, BOUND]).await?;
    for pair in [FAILED, BOUND] {
        seed::initializer(&journal, pair, "not_requested", "not_requested").await?;
        seed::unguarded(
            &journal,
            &[format!("UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received', start_state = 'started', result_state = 'confirmed', home_device_major = 0, home_device_minor = 1, home_inode = 1, home_mount_id = 1, work_device_major = 0, work_device_minor = 2, work_inode = 2, work_mount_id = 2 WHERE reservation_id = {}", pair.reservation)],
        )
        .await?;
    }
    Ok((journal, scope, engine))
}

fn helper(pair: Pair) -> UnboundInitializer {
    UnboundInitializer {
        name: pair.helper_name(),
        id: Some(pair.helper_id()),
        operation_key: format!("{}-1", pair.reservation - 1),
    }
}

async fn settle(
    scope: &ConnectionJournal,
    engine: &crate::launch::SelectedEngineContext,
    pair: Pair,
    helper: &UnboundInitializer,
) -> Result<(), crate::error::HostError> {
    scope
        .settle_removed_storage_initializer(
            pair.reservation,
            engine.engine_id(),
            engine.docker_root_digest(),
            helper,
        )
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_completion_removal_settles_only_its_own_reservations_initializer() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (journal, scope, engine) = seeded(dir.path()).await?;

    settle(&scope, &engine, FAILED, &helper(FAILED)).await?;

    assert_eq!(
        seed::text(&journal, STATES, FAILED.reservation).await?,
        "not_requested/absent"
    );
    assert_eq!(
        seed::text(&journal, STATES, BOUND.reservation).await?,
        "not_requested/not_requested"
    );
    assert!(
        scope
            .unretired_storage_initializer(
                FAILED.reservation,
                engine.engine_id(),
                engine.docker_root_digest()
            )
            .await?
            .is_none()
    );
    assert!(
        scope
            .unretired_storage_initializer(
                BOUND.reservation,
                engine.engine_id(),
                engine.docker_root_digest()
            )
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn settling_twice_and_after_a_crash_between_the_two_writes_converges() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (journal, scope, engine) = seeded(dir.path()).await?;
    seed::unguarded(
        &journal,
        &[format!("UPDATE capacity_reservation_storage_initialization SET abort_claim_generation = 1, abort_lease_until = 1, abort_helper_id = '{}', abort_state = 'remove_requested' WHERE reservation_id = 2", FAILED.helper_id())],
    )
    .await?;

    settle(&scope, &engine, FAILED, &helper(FAILED)).await?;
    settle(&scope, &engine, FAILED, &helper(FAILED)).await?;

    assert_eq!(
        seed::text(&journal, STATES, FAILED.reservation).await?,
        "not_requested/absent"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_helper_identity_that_differs_from_the_journal_settles_nothing() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (journal, scope, engine) = seeded(dir.path()).await?;
    for forged in [
        UnboundInitializer {
            id: Some(BOUND.helper_id()),
            ..helper(FAILED)
        },
        UnboundInitializer {
            name: BOUND.helper_name(),
            ..helper(FAILED)
        },
        UnboundInitializer {
            operation_key: "9-9".to_owned(),
            ..helper(FAILED)
        },
        UnboundInitializer {
            id: None,
            ..helper(FAILED)
        },
    ] {
        assert!(settle(&scope, &engine, FAILED, &forged).await.is_err());
    }
    assert!(
        settle(&scope, &engine, BOUND, &helper(FAILED))
            .await
            .is_err()
    );
    for pair in [FAILED, BOUND] {
        assert_eq!(
            seed::text(&journal, STATES, pair.reservation).await?,
            "not_requested/not_requested"
        );
    }
    Ok(())
}
