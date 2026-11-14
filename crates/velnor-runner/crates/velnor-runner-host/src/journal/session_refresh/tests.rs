// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "tests/docker.rs"]
mod docker;
#[path = "tests/seed.rs"]
mod seed;

use std::time::Duration;

use bollard::Docker;

use crate::config::RunnerPlatform;
use crate::journal::{ConnectionJournal, Journal, StorageRole};
use crate::launch::bootstrap::VerifiedCreatedStorageObject;
use crate::worker::ResourceBudgetConfig;
use crate::worker::storage_initialization::{
    StorageInitializationImage, StorageInitializationLimits, initialize, recover_interrupted,
};

use super::super::connection::storage::in_transaction;
use docker::{FixtureDocker, HELPER_ID};
use seed::{ENGINE_ID, IMAGE_ID, OPERATION_ID, RESERVATION_ID, Recorded, SOURCE_SHA};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn recorded(
    [reservation, intent, create_dispatch, create, abort]: [&str; 5],
    abort_helper_id: Option<&str>,
) -> Recorded {
    Recorded {
        reservation: reservation.to_owned(),
        intent: intent.to_owned(),
        create_dispatch: create_dispatch.to_owned(),
        create: create.to_owned(),
        abort: abort.to_owned(),
        abort_helper_id: abort_helper_id.map(str::to_owned),
    }
}

async fn kill_after_helper_created(
    docker: &Docker,
    scope: &ConnectionJournal,
    root: &str,
    fixture: &FixtureDocker,
) -> TestResult<()> {
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let limits = StorageInitializationLimits::from_reserved_budget(budget, 500, 64 << 20, 16)?;
    let image =
        StorageInitializationImage::for_test(IMAGE_ID, SOURCE_SHA, RunnerPlatform::LinuxAmd64);
    let metadata = VerifiedCreatedStorageObject::for_test(
        RESERVATION_ID,
        OPERATION_ID,
        ENGINE_ID,
        root,
        StorageRole::EngineMetadata,
        &seed::volume("-metadata"),
    );
    let initialization = initialize(
        docker,
        scope,
        RESERVATION_ID,
        &image,
        limits,
        Duration::from_secs(30),
        &metadata,
    );
    tokio::select! {
        _ = initialization => Err("the initialization ended before the daemon was killed".into()),
        () = fixture.helper_created() => Ok(()),
    }
}

async fn restart(journal: &Journal, docker: &Docker, scope: &ConnectionJournal) -> TestResult<()> {
    in_transaction(journal, async |connection| {
        super::recover_after_restart(connection).await
    })
    .await?;
    recover_interrupted(docker, scope).await?;
    Ok(())
}

#[tokio::test]
async fn restart_recovers_the_storage_initializer_of_a_killed_daemon() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, root) =
        seed::reservation_at_storage_initialization(&journal, &seed::binding()?, &docker).await?;
    kill_after_helper_created(&docker, &scope, &root, &fixture).await?;
    let stuck = recorded(
        [
            "reserved",
            "pending",
            "dispatched",
            "requested",
            "not_requested",
        ],
        None,
    );
    assert_eq!(seed::recorded(&journal).await?, stuck);

    fixture.take_requests();
    recover_interrupted(&docker, &scope).await?;
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert!(fixture.helper_present());
    assert_eq!(seed::recorded(&journal).await?, stuck);

    restart(&journal, &docker, &scope).await?;
    let requests = fixture.take_requests();
    assert!(requests.contains(&format!("GET /images/{IMAGE_ID}/json")));
    assert!(requests.contains(&format!("DELETE /containers/{HELPER_ID}")));
    assert!(!fixture.helper_present());
    let recovered = recorded(
        [
            "uncertain",
            "uncertain",
            "outcome_unknown",
            "uncertain",
            "absent",
        ],
        Some(HELPER_ID),
    );
    assert_eq!(seed::recorded(&journal).await?, recovered);

    restart(&journal, &docker, &scope).await?;
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert_eq!(seed::recorded(&journal).await?, recovered);
    Ok(())
}
