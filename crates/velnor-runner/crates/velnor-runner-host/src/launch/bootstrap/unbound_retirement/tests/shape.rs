// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

use super::fixture::FixtureDocker;
use super::seed;
use super::{FAILED, RESERVATION, RETIRED, TestResult, retire_unbound_pair};

const NOT_STARTED: &str = "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'not_requested', runner_container_id = NULL, dind_container_id = NULL WHERE reservation_id = {r}";
const NO_CONTAINERS: &str =
    "DELETE FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {r}";
const INITIALIZER: &str = "DROP TABLE capacity_reservation_storage_initialization; CREATE TABLE capacity_reservation_storage_initialization (reservation_id INTEGER, remove_state TEXT, abort_state TEXT); INSERT INTO capacity_reservation_storage_initialization VALUES ({r}, 'not_requested', '{abort}')";

fn statements(containers: bool, abort: Option<&str>) -> Vec<String> {
    let reservation = FAILED.reservation.to_string();
    let mut sql = vec![NOT_STARTED.to_owned()];
    if !containers {
        sql.push(NO_CONTAINERS.to_owned());
    }
    if let Some(abort) = abort {
        sql.extend(
            INITIALIZER
                .replace("{abort}", abort)
                .split("; ")
                .map(str::to_owned),
        );
    }
    sql.into_iter()
        .map(|statement| statement.replace("{r}", &reservation))
        .collect()
}

async fn claimed(sql: &[String]) -> TestResult<bool> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, _) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    seed::unguarded(&journal, sql).await?;
    Ok(scope
        .claim_unbound_retirement(FAILED.reservation)
        .await?
        .is_some())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_aborted_initializer_with_no_containers_is_claimed() -> TestResult<()> {
    assert!(claimed(&statements(false, Some("absent"))).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_that_never_started_without_an_initializer_row_is_not_claimed() -> TestResult<()> {
    assert!(!claimed(&statements(false, None)).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_initializer_that_was_not_proven_absent_is_not_claimed() -> TestResult<()> {
    for abort in ["not_requested", "requested", "uncertain"] {
        assert!(
            !claimed(&statements(false, Some(abort))).await?,
            "abort state {abort} was claimed"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn two_container_rows_on_a_pair_that_never_started_are_not_claimed() -> TestResult<()> {
    assert!(!claimed(&statements(true, Some("absent"))).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_started_pair_without_an_acquired_request_is_retired() -> TestResult<()> {
    let reservation = FAILED.reservation;
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(reservation).await?;
    let row = scope
        .physical_runner_reservations()
        .await?
        .into_iter()
        .find(|row| row.reservation_id == reservation)
        .ok_or("reservation missing")?;
    assert!(row.acquired_request.is_none());

    assert!(retire_unbound_pair(&docker, &scope, &engine, reservation).await?);

    assert_eq!(
        seed::text(&journal, RESERVATION, reservation).await?,
        RETIRED
    );
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());
    Ok(())
}
