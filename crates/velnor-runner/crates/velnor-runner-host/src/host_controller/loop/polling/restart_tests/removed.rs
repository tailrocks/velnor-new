// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use bollard::Docker;

use crate::config::HostConfig;
use crate::controller_ipc::ControlState;
use crate::host_controller::loop_driver::connection_poll::{
    ConnectionPollContext, fairness::FairnessTurn, poll_active_connections,
};
use crate::host_controller::loop_driver::setup::{RemovedStartup, reconcile_removed_at_startup};
use crate::journal::{CapacityLimits, Journal, OperationalCapacityLimits};
use crate::launch::SelectedEngineContext;
use crate::launch::bootstrap::unbound_pair_fixtures::{
    FAILED, RETIRED, fixture::FixtureDocker, seed,
};

use crate::host_controller::loop_driver::controller::startup::{FinishInputs, finish_startup};

use super::controller::Controller;
use super::{HOST_CONFIG, TestResult};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const UNCERTAIN: &str = "uncertain/00/uncertain/0/1/none/";

#[path = "removed/deadline.rs"]
mod deadline;

#[path = "removed/fairness.rs"]
mod fairness;

#[path = "removed/initializer_deadline.rs"]
mod initializer_deadline;

#[path = "removed/shared_budget.rs"]
mod shared_budget;

impl Controller {
    fn remove_connection(&mut self) -> TestResult<()> {
        let kept = HOST_CONFIG.replace("conn-test-1", "conn-other-1");
        self.config = HostConfig::parse(&kept)?;
        let directory = self.state.path().join("connection-drain");
        fs::create_dir(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let snapshot = self.prepared.binding.canonical_identity_bytes()?;
        let mut proof = format!("{TOKEN}\n").into_bytes();
        proof.extend_from_slice(&snapshot);
        for (name, bytes) in [
            ("conn-test-1", TOKEN.as_bytes()),
            ("conn-test-1~binding", snapshot.as_slice()),
            ("conn-test-1~drained", proof.as_slice()),
        ] {
            let path = directory.join(name);
            fs::write(&path, bytes)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    async fn poll_removed(
        self,
        journal: &Journal,
        docker: &Docker,
        engine: &SelectedEngineContext,
    ) -> TestResult<()> {
        let control_state = ControlState::new(&self.config);
        let context = ConnectionPollContext {
            config: &self.config,
            journal,
            docker,
            selected_engine: engine,
            worker_images: &self.images,
            state: self.state.path(),
            image_authority: &self.image_authority,
            admission_ready: true,
            control_state: &control_state,
            retirement_budget: Duration::from_secs(120),
        };
        let pending = vec![self.prepared.binding.clone()];
        let mut connections = vec![self.prepared];
        poll_active_connections(
            context,
            &pending,
            &mut connections,
            FairnessTurn::new(true, false),
        )
        .await?;
        Ok(())
    }

    async fn clean_up_at_startup(
        &self,
        journal: &Journal,
        docker: &Docker,
        engine: &SelectedEngineContext,
        budget: Duration,
    ) {
        reconcile_removed_at_startup(&RemovedStartup {
            state: self.state.path(),
            config: &self.config,
            journal,
            docker,
            engine,
            images: &self.images,
            image_authority: &self.image_authority,
            deadline: Instant::now() + budget,
        })
        .await;
    }
}

struct Restarted {
    _dir: tempfile::TempDir,
    fixture: FixtureDocker,
    docker: Docker,
    journal: Journal,
    engine: SelectedEngineContext,
}

async fn restarted_with_a_removed_connection() -> TestResult<(Restarted, Controller)> {
    restarted_with_removed_pairs(&[FAILED]).await
}

async fn restarted_with_removed_pairs(pairs: &[seed::Pair]) -> TestResult<(Restarted, Controller)> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, pairs)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, pairs).await?;
    journal.recover_after_restart_without_authority().await?;
    assert_eq!(
        seed::text(&journal, seed::RESERVATION, FAILED.reservation).await?,
        UNCERTAIN
    );
    let mut controller = Controller::new(&scope, &engine)?;
    controller.remove_connection()?;
    Ok((
        Restarted {
            _dir: dir,
            fixture,
            docker,
            journal,
            engine,
        },
        controller,
    ))
}

async fn assert_retired(restarted: &Restarted) -> TestResult<()> {
    assert_eq!(
        seed::text(&restarted.journal, seed::RESERVATION, FAILED.reservation).await?,
        RETIRED
    );
    assert_eq!(restarted.fixture.objects(FAILED), Vec::<String>::new());
    Ok(())
}

async fn bind_session(journal: &Journal, controller: &mut Controller) -> TestResult<()> {
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 2,
                cpu_millicores: 6_000,
                memory_bytes: 13 << 30,
            },
            OperationalCapacityLimits {
                cpu_millicores: 500,
                memory_bytes: 1 << 30,
            },
        )
        .await
        .map_err(|error| format!("capacity {error}"))?;
    let scope = &controller.prepared.scope;
    let nonce = "a".repeat(32);
    scope
        .prepare_session_create(&nonce)
        .await
        .map_err(|error| format!("prepare {error}"))?;
    let claim = scope
        .claim_session_create(&nonce)
        .await
        .map_err(|error| format!("claim {error}"))?
        .ok_or("session claim")?;
    let bound = scope
        .record_created_session(&claim, "session-test", 1)
        .await
        .map_err(|error| format!("record {error}"))?;
    controller.prepared.session_intent = bound.intent_id();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_connection_whose_poll_fails_has_its_unbound_pair_retired() -> TestResult<()> {
    let (restarted, mut controller) = restarted_with_a_removed_connection().await?;
    bind_session(&restarted.journal, &mut controller).await?;
    assert!(controller.prepared.credentials.is_none());
    controller
        .poll_removed(&restarted.journal, &restarted.docker, &restarted.engine)
        .await?;
    assert_retired(&restarted).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restart_retires_the_unbound_pair_of_a_removed_connection() -> TestResult<()> {
    let (restarted, controller) = restarted_with_a_removed_connection().await?;
    controller
        .clean_up_at_startup(
            &restarted.journal,
            &restarted.docker,
            &restarted.engine,
            Duration::from_secs(60),
        )
        .await;
    assert_retired(&restarted).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_connection_that_is_never_polled_has_its_unbound_pair_retired() -> TestResult<()>
{
    let (restarted, controller) = restarted_with_a_removed_connection().await?;
    assert_eq!(controller.prepared.session_intent, 0);
    controller
        .poll_removed(&restarted.journal, &restarted.docker, &restarted.engine)
        .await?;
    assert_retired(&restarted).await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_startup_sequence_retires_the_unbound_pair_of_a_removed_connection() -> TestResult<()> {
    let (restarted, controller) = restarted_with_a_removed_connection().await?;
    finish_startup(
        FinishInputs {
            current: &controller.config,
            state: controller.state.path(),
            docker: &restarted.docker,
            selected_engine: &restarted.engine,
            journal: &restarted.journal,
            image_authority: &controller.image_authority,
        },
        &controller.images,
    )
    .await?;
    assert_retired(&restarted).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_docker_cannot_hold_the_startup_reconcile_past_its_deadline() -> TestResult<()> {
    let (restarted, controller) = restarted_with_a_removed_connection().await?;
    restarted.fixture.hang_after(format!(
        "DELETE /containers/{}",
        FAILED.container_id("runner")
    ));
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(60),
        controller.clean_up_at_startup(
            &restarted.journal,
            &restarted.docker,
            &restarted.engine,
            Duration::from_millis(500),
        ),
    )
    .await?;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        seed::text(&restarted.journal, seed::RESERVATION, FAILED.reservation).await?,
        UNCERTAIN
    );
    Ok(())
}
