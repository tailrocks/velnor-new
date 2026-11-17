// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

use super::super::retire_uncertain_unbound_pairs;
use super::fixture::FixtureDocker;
use super::seed;
use super::{FAILED, RESERVATION, RETIRED, TestResult, retire_unbound_pair};

const NOT_STARTED: &str = "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'not_requested', runner_container_id = NULL, dind_container_id = NULL WHERE reservation_id = {r}";
const NO_CONTAINERS: &str =
    "DELETE FROM capacity_reservation_bootstrap_containers WHERE reservation_id = {r}";
const UNCLAIMED: &str = "UPDATE capacity_reservation_bootstrap_containers SET create_state = 'unclaimed', start_state = 'not_requested', container_id = NULL WHERE reservation_id = {r}";

#[derive(Clone, Copy)]
enum Containers {
    Started,
    None,
    Unclaimed,
}

fn statements(containers: Containers) -> Vec<String> {
    let reservation = FAILED.reservation.to_string();
    let mut sql = vec![NOT_STARTED];
    match containers {
        Containers::Started => {}
        Containers::None => sql.push(NO_CONTAINERS),
        Containers::Unclaimed => sql.push(UNCLAIMED),
    }
    sql.into_iter()
        .map(|statement| statement.replace("{r}", &reservation))
        .collect()
}

async fn shaped(journal: &Journal, containers: Containers, abort: Option<&str>) -> TestResult<()> {
    seed::unguarded(journal, &statements(containers)).await?;
    if let Some(abort) = abort {
        seed::initializer(journal, FAILED, "removed", abort).await?;
    }
    Ok(())
}

async fn claimed(containers: Containers, abort: Option<&str>) -> TestResult<bool> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, _) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    shaped(&journal, containers, abort).await?;
    Ok(scope
        .claim_unbound_retirement(FAILED.reservation)
        .await?
        .is_some())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_aborted_initializer_with_no_containers_is_claimed() -> TestResult<()> {
    assert!(claimed(Containers::None, Some("absent")).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_aborted_initializer_with_the_two_unclaimed_admission_rows_is_claimed() -> TestResult<()>
{
    assert!(claimed(Containers::Unclaimed, Some("absent")).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pristine_pair_without_dispatched_resources_is_not_claimed() -> TestResult<()> {
    for containers in [Containers::None, Containers::Unclaimed] {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("docker.sock");
        let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
        let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
        let journal = Journal::open(&dir.path().join("launch.db")).await?;
        let (scope, _) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
        scope.abandon_unbound_pair(FAILED.reservation).await?;
        shaped(&journal, containers, None).await?;
        seed::unguarded(&journal, &[
            format!("UPDATE capacity_reservation_provisioning_objects SET create_state='unclaimed',docker_id=NULL WHERE reservation_id={}", FAILED.reservation),
            format!("DELETE FROM capacity_reservation_networks WHERE reservation_id={}", FAILED.reservation),
        ]).await?;
        assert!(
            scope
                .claim_unbound_retirement(FAILED.reservation)
                .await?
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_initializer_that_was_not_proven_absent_is_not_claimed() -> TestResult<()> {
    for abort in ["not_requested", "requested", "uncertain"] {
        for containers in [Containers::None, Containers::Unclaimed] {
            assert!(
                !claimed(containers, Some(abort)).await?,
                "abort state {abort} was claimed"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn two_started_container_rows_on_a_pair_that_never_started_are_not_claimed() -> TestResult<()>
{
    assert!(!claimed(Containers::Started, Some("absent")).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_started_pair_with_no_container_ids_is_not_claimed() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, _) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    let reservation = FAILED.reservation.to_string();
    seed::unguarded(&journal, &[UNCLAIMED.replace("{r}", &reservation)]).await?;
    assert!(
        scope
            .claim_unbound_retirement(FAILED.reservation)
            .await?
            .is_none()
    );
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

#[tokio::test(flavor = "multi_thread")]
async fn a_no_container_aborted_reservation_is_listed_and_retired_by_the_sweep() -> TestResult<()> {
    sweep(Containers::None).await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unclaimed_admission_reservation_is_listed_and_retired_by_the_sweep() -> TestResult<()> {
    sweep(Containers::Unclaimed).await
}

async fn sweep(containers: Containers) -> TestResult<()> {
    let reservation = FAILED.reservation;
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    shaped(&journal, containers, Some("absent")).await?;

    scope.abandon_unbound_pair(reservation).await?;
    assert_eq!(scope.uncertain_unbound_pairs().await?, vec![reservation]);

    retire_uncertain_unbound_pairs(
        tokio::time::Instant::now() + std::time::Duration::from_secs(30),
        &docker,
        &scope,
        &engine,
    )
    .await?;

    assert_eq!(
        seed::text(&journal, RESERVATION, reservation).await?,
        RETIRED
    );
    assert_eq!(scope.uncertain_unbound_pairs().await?, Vec::<i64>::new());
    let _ = fixture;
    Ok(())
}
