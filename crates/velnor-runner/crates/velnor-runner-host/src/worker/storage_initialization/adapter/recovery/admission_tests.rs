// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use tokio::time::{Instant, advance, pause, resume, timeout_at};

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal},
    launch::bootstrap::unbound_pair_fixtures::{FAILED, fixture::FixtureDocker, seed},
};

use super::recover_absent;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const BUDGET: Duration = Duration::from_secs(2);
const RETRY_BUDGET: Duration = Duration::from_secs(30);
const INITIALIZER: &str = "SELECT abort_state || '/' || abort_claim_generation || '/' || abort_lease_until FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";
const LIVE_CLAIM: &str = "SELECT abort_state || '/' || abort_claim_generation || '/' || (abort_lease_until > CAST(strftime('%s', 'now') AS INTEGER)) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";

struct EligibleInitializer {
    _dir: tempfile::TempDir,
    fixture: FixtureDocker,
    docker: Docker,
    journal: Journal,
    scope: ConnectionJournal,
}

impl EligibleInitializer {
    async fn new() -> TestResult<Self> {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("docker.sock");
        let fixture = FixtureDocker::start(&socket, &[FAILED])?;
        let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
        let journal = Journal::open(&dir.path().join("launch.db")).await?;
        let (scope, _) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
        seed::initializer(&journal, FAILED, "not_requested", "not_requested").await?;
        seed::unguarded(
            &journal,
            &[
                format!(
                    "DELETE FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {}",
                    FAILED.reservation
                ),
                format!(
                    "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'not_requested', runner_container_id = NULL, dind_container_id = NULL WHERE reservation_id = {}",
                    FAILED.reservation
                ),
                format!(
                    "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received' WHERE reservation_id = {}",
                    FAILED.reservation
                ),
            ],
        )
        .await?;
        Ok(Self {
            _dir: dir,
            fixture,
            docker,
            journal,
            scope,
        })
    }

    async fn snapshot(&self) -> TestResult<(String, String, Vec<String>)> {
        Ok((
            seed::text(&self.journal, INITIALIZER, FAILED.reservation).await?,
            seed::text(&self.journal, seed::RESERVATION, FAILED.reservation).await?,
            self.fixture.objects(FAILED),
        ))
    }
}

#[tokio::test]
async fn initializer_candidate_contention_returns_at_its_deadline_without_claiming()
-> TestResult<()> {
    let initializer = EligibleInitializer::new().await?;
    let before = initializer.snapshot().await?;
    let guard = initializer.journal.write_guard().await;
    pause();
    let deadline = Instant::now() + BUDGET;
    let recovery = recover_absent(
        &initializer.docker,
        &initializer.scope,
        FAILED.reservation,
        deadline,
    );
    tokio::pin!(recovery);
    assert!(futures_util::poll!(&mut recovery).is_pending());
    advance(BUDGET + Duration::from_millis(1)).await;
    resume();
    let result = timeout_at(deadline + BUDGET, &mut recovery).await;
    assert!(matches!(result, Ok(Err(HostError::DockerTimeout))));
    drop(guard);
    assert_eq!(initializer.snapshot().await?, before);

    recover_absent(
        &initializer.docker,
        &initializer.scope,
        FAILED.reservation,
        Instant::now() + RETRY_BUDGET,
    )
    .await?;
    assert_eq!(
        seed::text(&initializer.journal, INITIALIZER, FAILED.reservation).await?,
        "absent/1/0"
    );
    assert_eq!(initializer.fixture.objects(FAILED), before.2);
    Ok(())
}

#[tokio::test]
async fn initializer_claim_contention_and_ready_lock_expiry_leave_no_abort_lease() -> TestResult<()>
{
    let initializer = EligibleInitializer::new().await?;
    let candidate = initializer
        .scope
        .storage_initializer_abort_candidate(FAILED.reservation)
        .await?
        .ok_or("initializer candidate")?;
    let before = initializer.snapshot().await?;
    for release_before_poll in [false, true] {
        let mut guard = Some(initializer.journal.write_guard().await);
        pause();
        let deadline = Instant::now() + BUDGET;
        let claim = initializer
            .scope
            .claim_storage_initializer_abort_before(&candidate, deadline);
        tokio::pin!(claim);
        assert!(futures_util::poll!(&mut claim).is_pending());
        advance(BUDGET + Duration::from_millis(1)).await;
        if release_before_poll {
            drop(guard.take());
        }
        resume();
        let result = timeout_at(deadline + BUDGET, &mut claim).await;
        assert!(
            matches!(result, Ok(Err(HostError::DockerTimeout))),
            "release_before_poll={release_before_poll}: {:?}",
            result
                .as_ref()
                .map(|inner| inner.as_ref().map(Option::is_some))
        );
        drop(guard);
        assert_eq!(initializer.snapshot().await?, before);
    }

    let claim = initializer
        .scope
        .claim_storage_initializer_abort_before(&candidate, Instant::now() + RETRY_BUDGET)
        .await?
        .ok_or("fresh initializer claim")?;
    assert_eq!(claim.abort_claim_generation(), 1);
    assert_eq!(
        seed::text(&initializer.journal, LIVE_CLAIM, FAILED.reservation).await?,
        "requested/1/1"
    );
    initializer
        .scope
        .mark_storage_initializer_abort_uncertain(&claim)
        .await?;
    assert_eq!(
        seed::text(&initializer.journal, INITIALIZER, FAILED.reservation).await?,
        "uncertain/1/0"
    );
    assert!(
        initializer
            .scope
            .storage_initializer_abort_candidate(FAILED.reservation)
            .await?
            .is_some()
    );
    assert_eq!(initializer.fixture.objects(FAILED), before.2);
    Ok(())
}
