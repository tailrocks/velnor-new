// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{Journal, UnboundRetirementPhase};

use super::fixture::FixtureDocker;
use super::seed::{self, RESERVATION};
use super::{FAILED, TestResult, retire_unbound_pair};

const PROGRESS: &str = "SELECT CAST(COUNT(*) AS TEXT) FROM capacity_reservation_unbound_retirement_progress WHERE reservation_id = ?1";

#[tokio::test(flavor = "multi_thread")]
async fn containers_still_started_cannot_be_recorded_absent() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, _) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    let claim = scope
        .claim_unbound_retirement(FAILED.reservation)
        .await?
        .ok_or("claim")?;

    let early = scope
        .record_unbound_retirement_phase(
            &claim,
            UnboundRetirementPhase::ContainersAbsent,
            &"d".repeat(64),
        )
        .await;

    assert!(early.is_err());
    assert_eq!(
        seed::text(&journal, PROGRESS, FAILED.reservation).await?,
        "0"
    );
    assert!(fixture.has_container(&FAILED.container_id("runner")));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_proven_removal_records_one_absence_per_container() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;

    assert!(retire_unbound_pair(&docker, &scope, &engine, FAILED.reservation).await?);

    assert_eq!(
        seed::text(&journal, "SELECT CAST(COUNT(*) AS TEXT) FROM unbound_container_absences WHERE reservation_id = ?1", FAILED.reservation).await?,
        "2"
    );
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        super::RETIRED
    );
    Ok(())
}
