// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "restart_tests/controller.rs"]
mod controller;
#[path = "restart_tests/docker.rs"]
mod docker;
#[path = "restart_tests/removed.rs"]
mod removed;
#[path = "restart_tests/seed.rs"]
mod seed;

use std::time::Duration;

use bollard::Docker;

use crate::config::RunnerPlatform;
use crate::journal::{ConnectionJournal, Journal, StorageRole};

use crate::launch::SelectedEngineContext;
use crate::launch::bootstrap::VerifiedCreatedStorageObject;
use crate::worker::ResourceBudgetConfig;
use crate::worker::storage_initialization::{
    StorageInitializationImage, StorageInitializationLimits, initialize,
};

use super::PositivePollContext;
use controller::Controller;
use docker::{FixtureDocker, HELPER_ID};
use seed::{ENGINE_ID, IMAGE_ID, LIVE_RESERVATIONS, PRIVATE_VOLUME};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
type Recorded = Vec<Option<String>>;

struct KillPoint {
    hang_on: String,
    stuck: [&'static str; 5],
    recovered: [&'static str; 5],
    helper_created: bool,
}

fn recorded(states: [&str; 5], abort: &str, abort_helper_id: Option<&str>) -> Recorded {
    states
        .into_iter()
        .chain([abort])
        .map(Some)
        .chain([abort_helper_id])
        .map(|value| value.map(str::to_owned))
        .collect()
}

async fn kill_at_hang(
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
        StorageInitializationImage::for_test(IMAGE_ID, &"1".repeat(40), RunnerPlatform::LinuxAmd64);
    let metadata_volume = format!("{PRIVATE_VOLUME}-metadata");
    let metadata = VerifiedCreatedStorageObject::for_test(
        2,
        1,
        ENGINE_ID,
        root,
        StorageRole::EngineMetadata,
        &metadata_volume,
    );
    let initialization = initialize(
        docker,
        scope,
        2,
        &image,
        limits,
        Duration::from_secs(30),
        &metadata,
    );
    tokio::select! {
        _ = initialization => Err("the initialization ended before the daemon was killed".into()),
        () = fixture.hung() => Ok(()),
    }
}

const HOST_CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

async fn restart(
    journal: &Journal,
    docker: &Docker,
    controller: &mut Controller,
    engine: &SelectedEngineContext,
) -> TestResult<()> {
    journal.recover_after_restart_without_authority().await?;
    controller.turn(journal, docker, engine).await
}

async fn live_reservations(journal: &Journal) -> TestResult<Vec<Recorded>> {
    let mut rows = Vec::new();
    for reservation in LIVE_RESERVATIONS {
        rows.push(seed::recorded(journal, reservation).await?);
    }
    Ok(rows)
}

async fn restart_recovers(kill: KillPoint) -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, kill.hang_on)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, root) = seed::reservation_at_storage_initialization(&journal, &docker).await?;
    let engine = SelectedEngineContext::capture(
        &scope.engine_context().await?.ok_or("engine context")?,
        &docker.info().await?,
    )?;
    kill_at_hang(&docker, &scope, &root, &fixture).await?;
    let mut controller = Controller::new(&scope, &engine)?;
    let helper = kill.helper_created.then_some(HELPER_ID);
    let stuck = recorded(kill.stuck, "not_requested", None);
    assert_eq!(seed::recorded(&journal, 2).await?, stuck);
    assert_eq!(fixture.helper_present(), kill.helper_created);
    let live = live_reservations(&journal).await?;

    fixture.take_requests();
    controller.turn(&journal, &docker, &engine).await?;
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert_eq!(seed::recorded(&journal, 2).await?, stuck);

    restart(&journal, &docker, &mut controller, &engine).await?;
    let requests = fixture.take_requests();
    let image_inspect = format!("GET /images/{IMAGE_ID}/json");
    let removal = format!("DELETE /containers/{HELPER_ID}");
    assert_eq!(requests.contains(&image_inspect), kill.helper_created);
    assert_eq!(requests.contains(&removal), kill.helper_created);
    assert!(!fixture.helper_present());
    let recovered = recorded(kill.recovered, "absent", helper);
    assert_eq!(live_reservations(&journal).await?, live);

    let mut volume_requests = requests
        .into_iter()
        .filter(|request| request.contains(" /volumes/"))
        .collect::<Vec<_>>();
    volume_requests.sort();
    let volume = |method: &str, role: &str| format!("{method} /volumes/{PRIVATE_VOLUME}{role}");
    let mut expected = [
        volume("DELETE", "-home"),
        volume("DELETE", "-metadata"),
        volume("DELETE", "-work"),
        volume("GET", ""),
        volume("GET", "-docker"),
    ]
    .into_iter()
    .chain(
        ["-home", "-metadata", "-work"]
            .into_iter()
            .flat_map(|role| std::iter::repeat_n(volume("GET", role), 4)),
    )
    .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(volume_requests, expected);
    let mut released = recovered;
    released[0] = Some("released".to_owned());
    assert_eq!(seed::recorded(&journal, 2).await?, released);
    assert_eq!(seed::cleanup_proven(&journal, 2).await?, (1, 1, 1));
    assert_eq!(live_reservations(&journal).await?, live);

    fixture.take_requests();
    restart(&journal, &docker, &mut controller, &engine).await?;
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert_eq!(seed::recorded(&journal, 2).await?, released);
    assert_eq!(live_reservations(&journal).await?, live);
    assert_eq!(controller.prepared.recovery_pass, 3);
    Ok(())
}

#[tokio::test]
async fn restart_recovers_a_crash_before_the_create_request() -> TestResult<()> {
    Box::pin(restart_recovers(KillPoint {
        hang_on: format!("GET /images/{IMAGE_ID}/json"),
        stuck: [
            "reserved",
            "pending",
            "not_dispatched",
            "requested",
            "not_requested",
        ],
        recovered: [
            "uncertain",
            "uncertain",
            "not_dispatched",
            "uncertain",
            "not_requested",
        ],
        helper_created: false,
    }))
    .await
}

#[tokio::test]
async fn restart_recovers_a_crash_while_the_create_request_is_in_flight() -> TestResult<()> {
    Box::pin(restart_recovers(KillPoint {
        hang_on: "POST /containers/create".to_owned(),
        stuck: [
            "reserved",
            "pending",
            "dispatched",
            "requested",
            "not_requested",
        ],
        recovered: [
            "uncertain",
            "uncertain",
            "outcome_unknown",
            "uncertain",
            "not_requested",
        ],
        helper_created: true,
    }))
    .await
}

#[tokio::test]
async fn restart_recovers_a_crash_after_docker_answered_the_create() -> TestResult<()> {
    Box::pin(restart_recovers(KillPoint {
        hang_on: format!("GET /containers/{HELPER_ID}/json"),
        stuck: [
            "reserved",
            "pending",
            "response_received",
            "response_recorded",
            "not_requested",
        ],
        recovered: [
            "uncertain",
            "uncertain",
            "response_received",
            "uncertain",
            "not_requested",
        ],
        helper_created: true,
    }))
    .await
}

#[tokio::test]
async fn restart_recovers_a_crash_while_the_helper_is_being_started() -> TestResult<()> {
    Box::pin(restart_recovers(KillPoint {
        hang_on: format!("POST /containers/{HELPER_ID}/attach"),
        stuck: [
            "reserved",
            "pending",
            "response_received",
            "created",
            "requested",
        ],
        recovered: [
            "uncertain",
            "uncertain",
            "response_received",
            "created",
            "uncertain",
        ],
        helper_created: true,
    }))
    .await
}
