// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;
use crate::launch::bootstrap::unbound_pair_fixtures::{fixture::FixtureDocker, seed, seed::Pair};

use super::{TestResult, bootstrap_seed, controller::Controller};
use bootstrap_seed::Crash;

#[path = "bootstrap/db.rs"]
mod db;
#[path = "bootstrap/guards.rs"]
mod guards;
#[path = "bootstrap/late.rs"]
mod late;

const CRASHED: Pair = Pair {
    reservation: 2,
    bound: false,
};
const NEIGHBOR: Pair = Pair {
    reservation: 4,
    bound: false,
};

struct Prepared {
    directory: tempfile::TempDir,
    fixture: FixtureDocker,
    docker: bollard::Docker,
    path: std::path::PathBuf,
    journal: Journal,
    scope: crate::journal::ConnectionJournal,
    engine: crate::launch::SelectedEngineContext,
    policy: String,
}

async fn prepare(crash: Crash, _foreign: bool) -> TestResult<Prepared> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[CRASHED, NEIGHBOR])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let path = dir.path().join("launch.db");
    let journal = Journal::open_historical_v48_for_test(&path)
        .await
        .map_err(|e| format!("initial opening journal: {e}"))?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[CRASHED, NEIGHBOR]).await?;
    bootstrap_seed::prepare(&journal, &scope, CRASHED, crash)
        .await
        .map_err(|e| format!("preparing {crash:?}: {e}"))?;
    crate::launch::bootstrap::unbound_pair_fixtures::fixture::bound_neighbor::install(
        &journal, NEIGHBOR, &engine,
    )
    .await?;
    bootstrap_seed::capacity(&journal).await?;
    let policy = bootstrap_seed::authority(&journal).await?;
    bootstrap_seed::bind_neighbor(&scope, NEIGHBOR, &policy).await?;
    if !matches!(crash, Crash::VolumeCreate | Crash::MetadataRecorded) {
        fixture.add_initializer(CRASHED);
    }
    external_prefix(&fixture, crash);
    if matches!(crash, Crash::NetworkRecorded) {
        let name = format!("velnor-v{}-private", CRASHED.nonce());
        let publication = fixture.delay_create("networks", &name, &CRASHED.network_id());
        publication.release();
        crate::launch::bootstrap::create_owned_network(
            &docker,
            &scope,
            &engine,
            CRASHED.reservation,
        )
        .await?;
        publication.published().await;
    }
    db::assert_historical(&journal, crash).await?;
    Ok(Prepared {
        directory: dir,
        fixture,
        docker,
        path,
        journal,
        scope,
        engine,
        policy,
    })
}

impl Prepared {
    async fn restart(self) -> TestResult<Self> {
        let Self {
            directory,
            fixture,
            docker,
            path,
            journal,
            scope,
            engine,
            policy,
        } = self;
        drop(scope);
        drop(journal);
        let journal = Journal::open(&path).await?;
        db::assert_current(&journal).await?;
        journal.recover_after_restart_without_authority().await?;
        let config = crate::config::HostConfig::parse(super::HOST_CONFIG)?;
        let scope = journal
            .connection_scope(config.connections().first().ok_or("binding")?)
            .await?;
        Ok(Self {
            directory,
            fixture,
            docker,
            path,
            journal,
            scope,
            engine,
            policy,
        })
    }
}

fn external_prefix(fixture: &FixtureDocker, crash: Crash) {
    match crash {
        Crash::DindCreate => fixture.remove_container(&CRASHED.container_id("runner")),
        Crash::VolumeCreate
        | Crash::NetworkCreate
        | Crash::MetadataRecorded
        | Crash::NetworkRecorded
        | Crash::InitializerUnfinished => {
            fixture.remove_container(&CRASHED.container_id("runner"));
            fixture.remove_container(&CRASHED.container_id("dind"));
        }
        Crash::RunnerCreate | Crash::Start => {}
    }
    if matches!(
        crash,
        Crash::VolumeCreate
            | Crash::MetadataRecorded
            | Crash::NetworkRecorded
            | Crash::InitializerUnfinished
    ) {
        fixture.remove_network(CRASHED);
    }
    if matches!(crash, Crash::MetadataRecorded) {
        for (name, role) in CRASHED.volumes() {
            if role != "engine-metadata" {
                fixture.remove_volume(&name);
            }
        }
    }
    if !matches!(crash, Crash::Start) {
        for role in ["runner", "dind"] {
            fixture.set_container_running(&CRASHED.container_id(role), false);
        }
    }
}

fn make_foreign(fixture: &FixtureDocker, crash: Crash) -> TestResult<()> {
    match crash {
        Crash::DindCreate => fixture.relabel_container(&CRASHED.container_id("dind")),
        Crash::VolumeCreate => fixture.relabel_volume(&CRASHED.volumes()[3].0),
        Crash::NetworkCreate => fixture.relabel_network(CRASHED),
        _ => return Err("foreign case requires an uncertain create".into()),
    }
    Ok(())
}

async fn before_restart(
    scope: &crate::journal::ConnectionJournal,
    journal: &Journal,
    docker: &bollard::Docker,
    engine: &crate::launch::SelectedEngineContext,
    fixture: &FixtureDocker,
    policy: &str,
) -> TestResult<()> {
    bootstrap_seed::excluded_from_other_recovery(scope, CRASHED.reservation, policy).await?;
    let capacity = journal.readiness_capacity_usage().await?;
    assert_eq!(capacity.jobs, 2);
    bootstrap_seed::assert_planning(scope, CRASHED.reservation, policy, true).await?;
    let mut controller = Controller::new(scope, engine)?;
    assert!(bootstrap_seed::charged(journal, CRASHED.reservation).await?);
    fixture.take_requests();
    controller.turn(journal, docker, engine).await?;
    assert!(bootstrap_seed::charged(journal, CRASHED.reservation).await?);
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    drop(controller);
    Ok(())
}

async fn recovers(crash: Crash, foreign: bool) -> TestResult<()> {
    let Prepared {
        directory: _directory,
        fixture,
        docker,
        path,
        journal,
        scope,
        engine,
        policy,
    } = prepare(crash, foreign).await?;
    if foreign {
        make_foreign(&fixture, crash)?;
    }
    let neighbor = bootstrap_seed::state(&journal, NEIGHBOR.reservation).await?;
    let neighbor_objects = fixture.objects(NEIGHBOR);
    Box::pin(before_restart(
        &scope, &journal, &docker, &engine, &fixture, &policy,
    ))
    .await?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path)
        .await
        .map_err(|e| format!("reopening journal: {e}"))?;
    db::assert_current(&journal).await?;
    journal
        .recover_after_restart_without_authority()
        .await
        .map_err(|e| format!("recovering {crash:?}: {e}"))?;
    let config = crate::config::HostConfig::parse(super::HOST_CONFIG)?;
    let binding = config.connections().first().ok_or("binding")?;
    let scope = journal.connection_scope(binding).await?;
    bootstrap_seed::excluded_from_other_recovery(&scope, CRASHED.reservation, &policy).await?;
    bootstrap_seed::assert_planning(&scope, CRASHED.reservation, &policy, true).await?;
    let mut controller = Controller::new(&scope, &engine)?;
    controller
        .turn(&journal, &docker, &engine)
        .await
        .map_err(|e| format!("controller {crash:?}: {e}"))?;
    assert_eq!(
        bootstrap_seed::charged(&journal, CRASHED.reservation).await?,
        foreign,
        "{crash:?} must release owned crash leftovers and retain foreign objects"
    );
    bootstrap_seed::assert_planning(&scope, CRASHED.reservation, &policy, foreign).await?;
    let recovered_capacity = journal.readiness_capacity_usage().await?;
    assert_eq!(recovered_capacity.jobs, if foreign { 2 } else { 1 });
    if foreign {
        foreign_retained(&fixture, crash)?;
    } else {
        assert_eq!(
            super::seed::cleanup_proven(&journal, CRASHED.reservation).await?,
            (1, 1, 1)
        );
        assert!(!fixture.has_initializer(CRASHED));
        assert!(
            fixture.objects(CRASHED).is_empty(),
            "{crash:?} left owned Docker objects"
        );
    }
    repeat(
        &journal,
        &docker,
        &engine,
        &fixture,
        &mut controller,
        &neighbor,
        &neighbor_objects,
    )
    .await?;
    bootstrap_seed::assert_planning(&scope, CRASHED.reservation, &policy, foreign).await?;
    if foreign {
        foreign_retained(&fixture, crash)?;
    }
    Ok(())
}

fn foreign_retained(fixture: &FixtureDocker, crash: Crash) -> TestResult<()> {
    let identity = match crash {
        Crash::DindCreate => CRASHED.container_id("dind"),
        Crash::VolumeCreate => CRASHED.volumes()[3].0.clone(),
        Crash::NetworkCreate => CRASHED.network_id(),
        _ => return Err("foreign case requires an uncertain create".into()),
    };
    assert!(fixture.objects(CRASHED).contains(&identity));
    Ok(())
}

async fn repeat(
    journal: &Journal,
    docker: &bollard::Docker,
    engine: &crate::launch::SelectedEngineContext,
    fixture: &FixtureDocker,
    controller: &mut Controller,
    neighbor: &str,
    neighbor_objects: &[String],
) -> TestResult<()> {
    let stable = bootstrap_seed::state(journal, CRASHED.reservation).await?;
    journal.recover_after_restart_without_authority().await?;
    controller.turn(journal, docker, engine).await?;
    assert_eq!(
        bootstrap_seed::state(journal, CRASHED.reservation).await?,
        stable
    );
    assert_eq!(
        bootstrap_seed::state(journal, NEIGHBOR.reservation).await?,
        neighbor
    );
    assert_eq!(fixture.objects(NEIGHBOR), neighbor_objects);
    assert!(
        !fixture
            .take_requests()
            .iter()
            .any(|request| request.contains("/create") || request.ends_with("/start"))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_releases_an_unknown_dind_create() -> TestResult<()> {
    Box::pin(recovers(Crash::DindCreate, false)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_releases_an_unknown_runner_create() -> TestResult<()> {
    Box::pin(recovers(Crash::RunnerCreate, false)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_stops_a_pair_with_an_unknown_start() -> TestResult<()> {
    Box::pin(recovers(Crash::Start, false)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_releases_an_unknown_volume_create() -> TestResult<()> {
    Box::pin(recovers(Crash::VolumeCreate, false)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_releases_an_unknown_network_create() -> TestResult<()> {
    Box::pin(recovers(Crash::NetworkCreate, false)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_releases_recorded_metadata_before_other_creates() -> TestResult<()> {
    Box::pin(recovers(Crash::MetadataRecorded, false)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_releases_recorded_network_before_container_create() -> TestResult<()> {
    Box::pin(recovers(Crash::NetworkRecorded, false)).await
}
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_restart_retains_foreign_objects_at_the_deterministic_names() -> TestResult<()> {
    for crash in [Crash::DindCreate, Crash::VolumeCreate, Crash::NetworkCreate] {
        Box::pin(recovers(crash, true)).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unfinished_initializer_is_not_claimed_by_early_resource_retirement() -> TestResult<()> {
    let case = prepare(Crash::InitializerUnfinished, false)
        .await?
        .restart()
        .await?;
    case.scope.abandon_unbound_pair(CRASHED.reservation).await?;
    assert!(
        case.scope
            .claim_unbound_retirement(CRASHED.reservation)
            .await?
            .is_none()
    );
    assert!(bootstrap_seed::charged(&case.journal, CRASHED.reservation).await?);
    assert!(case.fixture.has_initializer(CRASHED));
    assert_eq!(db::generation(&case.journal).await?, 0);
    Ok(())
}
