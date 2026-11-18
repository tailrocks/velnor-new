// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const CONNECTION_ID: &str = "conn-test-1";
const ENGINE_ID: &str = "engine-test";
const SCALE_SET_ID: i64 = 7;

const BINDING_DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SOURCE_DIGEST: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const ACQUIRE_DIGEST: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const ROOT_DIGEST: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const INSTANCE_DIGEST: &str = "5555555555555555555555555555555555555555555555555555555555555555";
const SNAPSHOT_DIGEST: &str = "6666666666666666666666666666666666666666666666666666666666666666";

const CLAIMED_INTENT: i64 = 10;
const CLAIMED_RESERVATION: i64 = 20;
const CLAIMED_REQUEST: i64 = 7;
const CLAIMED_RUNNER_ID: i64 = 42;
const CLAIMED_RUNNER_NAME: &str = "runner";

const FREE_INTENT: i64 = 11;
const FREE_RESERVATION: i64 = 21;
const FREE_REQUEST: i64 = 8;
const FREE_RUNNER_ID: i64 = 43;
const FREE_RUNNER_NAME: &str = "runner-11";

fn binding() -> TestResult<RepositoryConnection> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse(CONNECTION_ID)?,
        42,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-test-1")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            vec!["main".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )?)
}

fn intent(id: i64, subject: &str, runner_name: &str) -> String {
    format!(
        "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key, runner_name) VALUES ({id}, '{CONNECTION_ID}', 'launch', '{subject}', 'pending', {SCALE_SET_ID}, '{ENGINE_ID}', 'provisioning_held', 'assign-{id}', '{runner_name}')"
    )
}

fn reservation(id: i64, intent: i64) -> String {
    format!(
        "INSERT INTO capacity_reservations (id, intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES ({id}, {intent}, '{CONNECTION_ID}', 'assign-{intent}', '{ENGINE_ID}', '{ROOT_DIGEST}', '{BINDING_DIGEST}', '{id}', 3000, 6442450944, 'reserved')"
    )
}

fn lineage(id: i64) -> String {
    format!(
        "INSERT INTO physical_attempt_lineages (lineage_id, connection_id, scale_set_id, binding_digest) VALUES ({id}, '{CONNECTION_ID}', {SCALE_SET_ID}, '{BINDING_DIGEST}')"
    )
}

fn attempt(reservation: i64, intent: i64, request: i64) -> String {
    format!(
        "INSERT INTO physical_attempts (lineage_id, generation, reservation_id, intent_id, physical_nonce, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, acquire_response_digest, engine_id, docker_root_digest, created_at) VALUES ({reservation}, 1, {reservation}, {intent}, '{reservation:032x}', '{CONNECTION_ID}', {SCALE_SET_ID}, {request}, '{BINDING_DIGEST}', '{SOURCE_DIGEST}', '{ACQUIRE_DIGEST}', '{ENGINE_ID}', '{ROOT_DIGEST}', 0)"
    )
}

fn authorization(request: i64) -> String {
    format!(
        "INSERT INTO connection_request_authorizations (connection_id, scale_set_id, request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json, acquire_state, acquire_response_json, acquire_response_digest) VALUES ('{CONNECTION_ID}', {SCALE_SET_ID}, {request}, 0, 'branch_restricted', 'created_empty_exclusive_admin_session_v1', '{BINDING_DIGEST}', '{SOURCE_DIGEST}', '{{}}', 'acquired', '{{}}', '{ACQUIRE_DIGEST}')"
    )
}

fn runner_jit(intent: i64, request: i64, runner_id: i64, runner_name: &str) -> String {
    format!(
        "INSERT INTO connection_runner_jits (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name) VALUES ({intent}, '{CONNECTION_ID}', {SCALE_SET_ID}, {request}, '{runner_name}', 'ready', {runner_id}, '{runner_name}')"
    )
}

fn retirement_claim(reservation: i64, intent: i64, request: i64, runner_id: i64) -> String {
    format!(
        "INSERT INTO physical_attempt_post_jit_retirement_claims (reservation_id, claim_generation, lineage_id, generation, intent_id, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, acquire_response_digest, engine_id, docker_root_digest, instance_binding_digest, runner_id, runner_name, snapshot_sha256, lease_until, created_at) VALUES ({reservation}, 1, {reservation}, 1, {intent}, '{CONNECTION_ID}', {SCALE_SET_ID}, {request}, '{BINDING_DIGEST}', '{SOURCE_DIGEST}', '{ACQUIRE_DIGEST}', '{ENGINE_ID}', '{ROOT_DIGEST}', '{INSTANCE_DIGEST}', {runner_id}, '{CLAIMED_RUNNER_NAME}', '{SNAPSHOT_DIGEST}', 4102444800, 0)"
    )
}

async fn run(journal: &Journal, statement: &str) -> TestResult<()> {
    journal.connection().await?.execute(statement, ()).await?;
    Ok(())
}

async fn journal_with_two_ready_runners(dir: &tempfile::TempDir) -> TestResult<Journal> {
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine(ENGINE_ID).await?;
    journal.connection_scope(&binding()?).await?;
    for statement in [
        intent(CLAIMED_INTENT, "req-claimed", CLAIMED_RUNNER_NAME),
        intent(FREE_INTENT, "req-free", FREE_RUNNER_NAME),
        reservation(CLAIMED_RESERVATION, CLAIMED_INTENT),
        reservation(FREE_RESERVATION, FREE_INTENT),
        lineage(CLAIMED_RESERVATION),
        lineage(FREE_RESERVATION),
        authorization(CLAIMED_REQUEST),
        authorization(FREE_REQUEST),
        attempt(CLAIMED_RESERVATION, CLAIMED_INTENT, CLAIMED_REQUEST),
        attempt(FREE_RESERVATION, FREE_INTENT, FREE_REQUEST),
        runner_jit(
            CLAIMED_INTENT,
            CLAIMED_REQUEST,
            CLAIMED_RUNNER_ID,
            CLAIMED_RUNNER_NAME,
        ),
        runner_jit(FREE_INTENT, FREE_REQUEST, FREE_RUNNER_ID, FREE_RUNNER_NAME),
    ] {
        run(&journal, &statement).await?;
    }
    Ok(journal)
}

async fn journal_with_a_retirement_claim(dir: &tempfile::TempDir) -> TestResult<Journal> {
    let journal = journal_with_two_ready_runners(dir).await?;
    run(
        &journal,
        "DROP TRIGGER physical_attempt_post_jit_retirement_claim_insert_guard",
    )
    .await?;
    run(
        &journal,
        &retirement_claim(
            CLAIMED_RESERVATION,
            CLAIMED_INTENT,
            CLAIMED_REQUEST,
            CLAIMED_RUNNER_ID,
        ),
    )
    .await?;
    Ok(journal)
}

async fn state(journal: &Journal, intent: i64) -> TestResult<String> {
    let mut rows = journal
        .connection()
        .await?
        .query(
            "SELECT state FROM connection_runner_jits WHERE intent_id = ?1",
            (intent,),
        )
        .await?;
    let row = rows.next().await?.ok_or("missing runner jit row")?;
    Ok(row.get(0)?)
}

#[tokio::test]
async fn a_runner_start_after_a_retirement_claim_is_refused() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = journal_with_a_retirement_claim(&dir).await?;

    let started = journal
        .connection()
        .await?
        .execute(
            "UPDATE connection_runner_jits SET state = 'started', started_request_id = ?1 WHERE intent_id = ?2",
            (CLAIMED_REQUEST, CLAIMED_INTENT),
        )
        .await;

    assert!(started.is_err());
    assert_eq!(state(&journal, CLAIMED_INTENT).await?, "ready");
    Ok(())
}

#[tokio::test]
async fn a_runner_claim_row_after_a_retirement_claim_is_refused() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = journal_with_a_retirement_claim(&dir).await?;
    run(
        &journal,
        "DELETE FROM connection_runner_jits WHERE intent_id = 10",
    )
    .await?;

    let claimed_row = journal
        .connection()
        .await?
        .execute(
            "INSERT INTO connection_runner_jits (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state) VALUES (?1, ?2, ?3, ?4, ?5, 'claimed')",
            (
                CLAIMED_INTENT,
                CONNECTION_ID,
                SCALE_SET_ID,
                CLAIMED_REQUEST,
                CLAIMED_RUNNER_NAME,
            ),
        )
        .await;

    assert!(claimed_row.is_err());
    Ok(())
}

#[tokio::test]
async fn a_runner_start_without_a_retirement_claim_is_recorded() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = journal_with_two_ready_runners(&dir).await?;

    journal
        .connection()
        .await?
        .execute(
            "UPDATE connection_runner_jits SET state = 'started', started_request_id = ?1 WHERE intent_id = ?2",
            (FREE_REQUEST, FREE_INTENT),
        )
        .await?;

    assert_eq!(state(&journal, FREE_INTENT).await?, "started");
    Ok(())
}
