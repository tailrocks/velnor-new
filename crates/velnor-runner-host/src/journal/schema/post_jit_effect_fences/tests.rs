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
const FENCE_MESSAGE: &str = "A retirement claim prevents this operation.";

const BINDING_DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SOURCE_DIGEST: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const ACQUIRE_DIGEST: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const ROOT_DIGEST: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const INSTANCE_DIGEST: &str = "5555555555555555555555555555555555555555555555555555555555555555";
const SNAPSHOT_DIGEST: &str = "6666666666666666666666666666666666666666666666666666666666666666";

const READY_INTENT: i64 = 10;
const READY_RESERVATION: i64 = 20;
const READY_REQUEST: i64 = 7;
const READY_RUNNER_ID: i64 = 42;
const READY_RUNNER_NAME: &str = "runner";

const FREE_INTENT: i64 = 11;
const FREE_RESERVATION: i64 = 21;
const FREE_REQUEST: i64 = 8;
const FREE_RUNNER_ID: i64 = 43;
const FREE_RUNNER_NAME: &str = "runner-11";

const BOUND_INTENT: i64 = 12;
const BOUND_RESERVATION: i64 = 22;
const BOUND_REQUEST: i64 = 9;
const BOUND_RUNNER_ID: i64 = 44;
const BOUND_RUNNER_NAME: &str = "runner-12";

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

fn intent(id: i64, runner_name: &str) -> String {
    format!(
        "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, docker_engine_id, launch_phase, assignment_key, runner_name) VALUES ({id}, '{CONNECTION_ID}', 'launch', 'req-{id}', 'pending', {SCALE_SET_ID}, '{ENGINE_ID}', 'provisioning_held', 'assign-{id}', '{runner_name}')"
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

fn claimed_runner_jit(intent: i64, request: i64, runner_name: &str) -> String {
    format!(
        "INSERT INTO connection_runner_jits (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state) VALUES ({intent}, '{CONNECTION_ID}', {SCALE_SET_ID}, {request}, '{runner_name}', 'claimed')"
    )
}

fn bound_runner_jit(intent: i64, request: i64, runner_id: i64, runner_name: &str) -> String {
    format!(
        "UPDATE connection_runner_jits SET state = 'bound', runner_id = {runner_id}, runner_name = '{runner_name}' WHERE intent_id = {intent} AND connection_id = '{CONNECTION_ID}' AND scale_set_id = {SCALE_SET_ID} AND witness_request_id = {request} AND requested_name = '{runner_name}' AND state = 'claimed' AND runner_id IS NULL AND runner_name IS NULL"
    )
}

fn ready_runner_jit(intent: i64, runner_id: i64, runner_name: &str) -> String {
    format!(
        "UPDATE connection_runner_jits SET state = 'ready' WHERE intent_id = {intent} AND connection_id = '{CONNECTION_ID}' AND scale_set_id = {SCALE_SET_ID} AND state = 'bound' AND runner_id = {runner_id} AND runner_name = '{runner_name}'"
    )
}

fn start_runner_jit(intent: i64, request: i64, state: &str) -> String {
    format!(
        "UPDATE connection_runner_jits SET state = '{state}', started_request_id = {request} WHERE intent_id = {intent}"
    )
}

fn retirement_claim(
    reservation: i64,
    intent: i64,
    request: i64,
    runner_id: i64,
    runner_name: &str,
) -> String {
    format!(
        "INSERT INTO physical_attempt_post_jit_retirement_claims (reservation_id, claim_generation, lineage_id, generation, intent_id, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, acquire_response_digest, engine_id, docker_root_digest, instance_binding_digest, runner_id, runner_name, snapshot_sha256, lease_until, created_at) VALUES ({reservation}, 1, {reservation}, 1, {intent}, '{CONNECTION_ID}', {SCALE_SET_ID}, {request}, '{BINDING_DIGEST}', '{SOURCE_DIGEST}', '{ACQUIRE_DIGEST}', '{ENGINE_ID}', '{ROOT_DIGEST}', '{INSTANCE_DIGEST}', {runner_id}, '{runner_name}', '{SNAPSHOT_DIGEST}', 4102444800, 0)"
    )
}

async fn run(journal: &Journal, statement: &str) -> TestResult<()> {
    journal.connection().await?.execute(statement, ()).await?;
    Ok(())
}

async fn refused(journal: &Journal, statement: &str) -> TestResult<()> {
    let error = journal
        .connection()
        .await?
        .execute(statement, ())
        .await
        .err()
        .ok_or("the retirement claim fence accepted the change")?;
    assert!(
        error.to_string().contains(FENCE_MESSAGE),
        "unexpected error: {error}"
    );
    Ok(())
}

async fn seed_intent(
    journal: &Journal,
    id: i64,
    reservation_id: i64,
    request: i64,
    runner_id: i64,
    runner_name: &str,
    ready: bool,
) -> TestResult<()> {
    for statement in [
        intent(id, runner_name),
        reservation(reservation_id, id),
        lineage(reservation_id),
        authorization(request),
        attempt(reservation_id, id, request),
        claimed_runner_jit(id, request, runner_name),
        bound_runner_jit(id, request, runner_id, runner_name),
    ] {
        run(journal, &statement).await?;
    }
    if ready {
        run(journal, &ready_runner_jit(id, runner_id, runner_name)).await?;
    }
    Ok(())
}

async fn journal_with_runners(dir: &tempfile::TempDir) -> TestResult<Journal> {
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine(ENGINE_ID).await?;
    journal.connection_scope(&binding()?).await?;
    seed_intent(
        &journal,
        READY_INTENT,
        READY_RESERVATION,
        READY_REQUEST,
        READY_RUNNER_ID,
        READY_RUNNER_NAME,
        true,
    )
    .await?;
    seed_intent(
        &journal,
        FREE_INTENT,
        FREE_RESERVATION,
        FREE_REQUEST,
        FREE_RUNNER_ID,
        FREE_RUNNER_NAME,
        true,
    )
    .await?;
    seed_intent(
        &journal,
        BOUND_INTENT,
        BOUND_RESERVATION,
        BOUND_REQUEST,
        BOUND_RUNNER_ID,
        BOUND_RUNNER_NAME,
        false,
    )
    .await?;
    Ok(journal)
}

async fn journal_with_retirement_claims(dir: &tempfile::TempDir) -> TestResult<Journal> {
    let journal = journal_with_runners(dir).await?;
    run(
        &journal,
        "DROP TRIGGER physical_attempt_post_jit_retirement_claim_insert_guard",
    )
    .await?;
    run(
        &journal,
        &retirement_claim(
            READY_RESERVATION,
            READY_INTENT,
            READY_REQUEST,
            READY_RUNNER_ID,
            READY_RUNNER_NAME,
        ),
    )
    .await?;
    run(
        &journal,
        &retirement_claim(
            BOUND_RESERVATION,
            BOUND_INTENT,
            BOUND_REQUEST,
            BOUND_RUNNER_ID,
            BOUND_RUNNER_NAME,
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

async fn rows(journal: &Journal, intent: i64) -> TestResult<i64> {
    let mut rows = journal
        .connection()
        .await?
        .query(
            "SELECT COUNT(*) FROM connection_runner_jits WHERE intent_id = ?1",
            (intent,),
        )
        .await?;
    let row = rows.next().await?.ok_or("missing runner jit count")?;
    Ok(row.get(0)?)
}

#[tokio::test]
async fn a_runner_start_after_a_retirement_claim_is_refused() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = journal_with_retirement_claims(&dir).await?;

    for (intent, request) in [(READY_INTENT, READY_REQUEST), (BOUND_INTENT, BOUND_REQUEST)] {
        for new_state in ["start_requested", "start_uncertain", "ready", "started"] {
            refused(&journal, &start_runner_jit(intent, request, new_state)).await?;
        }
    }

    assert_eq!(state(&journal, READY_INTENT).await?, "ready");
    assert_eq!(state(&journal, BOUND_INTENT).await?, "bound");
    Ok(())
}

#[tokio::test]
async fn a_runner_claim_row_after_a_retirement_claim_is_refused() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = journal_with_retirement_claims(&dir).await?;
    run(
        &journal,
        &format!("DELETE FROM connection_runner_jits WHERE intent_id = {READY_INTENT}"),
    )
    .await?;

    refused(
        &journal,
        &claimed_runner_jit(READY_INTENT, READY_REQUEST, READY_RUNNER_NAME),
    )
    .await?;
    assert_eq!(rows(&journal, READY_INTENT).await?, 0);

    run(
        &journal,
        &format!("DELETE FROM connection_runner_jits WHERE intent_id = {FREE_INTENT}"),
    )
    .await?;
    run(
        &journal,
        &claimed_runner_jit(FREE_INTENT, FREE_REQUEST, FREE_RUNNER_NAME),
    )
    .await?;
    assert_eq!(state(&journal, FREE_INTENT).await?, "claimed");
    Ok(())
}

#[tokio::test]
async fn a_runner_start_without_a_retirement_claim_is_recorded() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = journal_with_runners(&dir).await?;

    run(
        &journal,
        &start_runner_jit(FREE_INTENT, FREE_REQUEST, "started"),
    )
    .await?;

    assert_eq!(state(&journal, FREE_INTENT).await?, "started");
    Ok(())
}
