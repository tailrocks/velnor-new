// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;
use crate::journal::connection::reservation_release_ready;
use crate::launch::bootstrap::unbound_pair_fixtures::fixture::FixtureDocker;
use crate::launch::bootstrap::unbound_pair_fixtures::{FAILED, seed};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const QUIESCE: [&str; 5] = [
    "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'not_requested', runner_container_id = NULL, dind_container_id = NULL WHERE reservation_id = 2",
    "DELETE FROM capacity_reservation_provisioning_objects WHERE reservation_id = 2",
    "DELETE FROM capacity_reservation_bootstrap_containers WHERE reservation_id = 2",
    "DELETE FROM capacity_reservation_networks WHERE reservation_id = 2",
    "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received', start_state = 'started', result_state = 'confirmed', home_device_major = 0, home_device_minor = 1, home_inode = 1, home_mount_id = 1, work_device_major = 0, work_device_minor = 2, work_inode = 2, work_mount_id = 2 WHERE reservation_id = 2",
];

async fn ready(journal: &Journal) -> TestResult<bool> {
    let connection = journal.connection().await?;
    Ok(reservation_release_ready(&connection, FAILED.reservation).await?)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reservation_is_release_ready_once_completion_settles_its_removed_initializer()
-> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    seed::initializer(&journal, FAILED, "not_requested", "not_requested").await?;
    seed::unguarded(&journal, &QUIESCE.map(str::to_owned)).await?;
    let helper = scope
        .unretired_storage_initializer(
            FAILED.reservation,
            engine.engine_id(),
            engine.docker_root_digest(),
        )
        .await?
        .ok_or("initializer row missing")?;

    assert!(!ready(&journal).await?);
    scope
        .settle_removed_storage_initializer(
            FAILED.reservation,
            engine.engine_id(),
            engine.docker_root_digest(),
            &helper,
        )
        .await?;
    assert!(ready(&journal).await?);
    Ok(())
}
