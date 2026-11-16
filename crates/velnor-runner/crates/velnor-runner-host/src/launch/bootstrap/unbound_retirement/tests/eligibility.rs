// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

use super::fixture::FixtureDocker;
use super::seed::{self, RESERVATION};
use super::{FAILED, TestResult, retire_unbound_pair};

#[tokio::test(flavor = "multi_thread")]
async fn a_jit_requested_after_the_abandon_is_not_retired() -> TestResult<()> {
    refused("UPDATE connection_intents SET jit_requested = 1 WHERE id = {op}").await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reservation_with_a_cleanup_proof_is_not_retired() -> TestResult<()> {
    refused("UPDATE capacity_reservations SET remote_cleanup_proven = 1 WHERE id = {r}").await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_with_a_publication_state_is_not_retired() -> TestResult<()> {
    refused(
        "UPDATE capacity_reservation_provisioning_state SET publication_state = 'requested' WHERE reservation_id = {r}",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_runner_jit_row_is_not_retired() -> TestResult<()> {
    refused(
        "DROP TABLE connection_runner_jits; CREATE TABLE connection_runner_jits (intent_id INTEGER); INSERT INTO connection_runner_jits VALUES ({op})",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_execution_proof_is_not_retired() -> TestResult<()> {
    refused(
        "DROP TABLE connection_execution_proofs; CREATE TABLE connection_execution_proofs (intent_id INTEGER); INSERT INTO connection_execution_proofs VALUES ({op})",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_execution_decision_is_not_retired() -> TestResult<()> {
    refused(
        "DROP TABLE connection_execution_decisions; CREATE TABLE connection_execution_decisions (intent_id INTEGER); INSERT INTO connection_execution_decisions VALUES ({op})",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_publication_operation_is_not_retired() -> TestResult<()> {
    refused(
        "DROP TABLE execution_publication_operations; CREATE TABLE execution_publication_operations (reservation_id INTEGER); INSERT INTO execution_publication_operations VALUES ({r})",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_container_that_was_not_created_is_not_retired() -> TestResult<()> {
    refused(
        "UPDATE capacity_reservation_bootstrap_containers SET create_state = 'uncertain', container_id = NULL, start_state = 'not_requested' WHERE reservation_id = {r} AND role = 'dind'",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_object_still_requested_is_not_retired() -> TestResult<()> {
    refused(
        "UPDATE capacity_reservation_provisioning_objects SET create_state = 'requested', docker_id = NULL WHERE reservation_id = {r} AND role = 'shared-run'",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pre_jit_cleanup_claim_is_not_retired() -> TestResult<()> {
    refused(
        "DROP TABLE capacity_reservation_pre_jit_cleanup_claims; CREATE TABLE capacity_reservation_pre_jit_cleanup_claims (reservation_id INTEGER); INSERT INTO capacity_reservation_pre_jit_cleanup_claims VALUES ({r})",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_post_jit_retirement_claim_is_not_retired() -> TestResult<()> {
    refused(
        "DROP TABLE physical_attempt_post_jit_retirement_claims; CREATE TABLE physical_attempt_post_jit_retirement_claims (reservation_id INTEGER); INSERT INTO physical_attempt_post_jit_retirement_claims VALUES ({r})",
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_current_attempt_is_not_retired() -> TestResult<()> {
    refused("DELETE FROM physical_attempt_current WHERE reservation_id = {r}").await
}

async fn refused(sql: &str) -> TestResult<()> {
    let reservation = FAILED.reservation;
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(reservation).await?;
    let statements = sql
        .replace("{r}", &reservation.to_string())
        .replace("{op}", &(reservation - 1).to_string())
        .split("; ")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    seed::unguarded(&journal, &statements).await?;
    let before = fixture.objects(FAILED);
    let state = seed::text(&journal, RESERVATION, reservation).await?;
    fixture.take_requests();

    let retired = retire_unbound_pair(&docker, &scope, &engine, reservation).await?;

    assert!(!retired);
    assert_eq!(fixture.take_requests(), Vec::<String>::new());
    assert_eq!(fixture.objects(FAILED), before);
    assert_eq!(seed::text(&journal, RESERVATION, reservation).await?, state);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_storage_initialization_does_not_block_the_retirement() -> TestResult<()> {
    retired_despite("removed", "not_requested").await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_absent_storage_initialization_helper_does_not_block_the_retirement() -> TestResult<()> {
    retired_despite("requested", "absent").await
}

async fn retired_despite(remove_state: &str, abort_state: &str) -> TestResult<()> {
    let reservation = FAILED.reservation;
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(reservation).await?;
    seed::initializer(&journal, FAILED, remove_state, abort_state).await?;

    assert!(retire_unbound_pair(&docker, &scope, &engine, reservation).await?);
    assert_eq!(fixture.objects(FAILED), Vec::<String>::new());
    Ok(())
}
