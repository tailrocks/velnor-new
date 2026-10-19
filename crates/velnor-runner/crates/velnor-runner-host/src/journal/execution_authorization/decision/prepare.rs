//! Bind the hook proof to the exact native Started request and prepare one decision.

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::execution_authorization::records::{ExecutionProofProgress, digest};
use crate::launch::execution_authorization::{
    AuthorizedExecutionSource, MAX_PROOF_BYTES, PersistedExecutionDecision,
    PersistedExecutionSource,
};

use super::proof_row::{ProofRow, load_proof};
use super::started::{self, StartedExecution};

mod decision;

pub(in crate::journal::execution_authorization) async fn prepare(
    scope: &ConnectionJournal,
    source: &AuthorizedExecutionSource,
) -> Result<ExecutionProofProgress, HostError> {
    source.validate()?;
    let operation_id = source.request().intent_id();
    if operation_id <= 0 {
        return Err(HostError::Identity);
    }
    let context = PrepareContext::new(scope, source, operation_id)?;
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        prepare_in_transaction(connection, &context).await
    })
    .await
}

struct PrepareContext<'a> {
    source: &'a AuthorizedExecutionSource,
    operation_id: i64,
    reservation_id: i64,
    connection_id: String,
    source_json: String,
    source_digest: String,
    request_digest: String,
}

impl<'a> PrepareContext<'a> {
    fn new(
        scope: &ConnectionJournal,
        source: &'a AuthorizedExecutionSource,
        operation_id: i64,
    ) -> Result<Self, HostError> {
        let source_bytes = source.canonical_json()?;
        let source_json =
            String::from_utf8(source_bytes.clone()).map_err(|_| HostError::Journal)?;
        Ok(Self {
            source,
            operation_id,
            reservation_id: source.request().reservation_id(),
            connection_id: scope.connection_id().as_str().to_owned(),
            source_json,
            source_digest: digest(&source_bytes),
            request_digest: source.request().sha256()?,
        })
    }
}

enum PrepareState {
    Progress(ExecutionProofProgress),
    Ready {
        proof: Box<ProofRow>,
        started: StartedExecution,
    },
}

async fn prepare_in_transaction(
    connection: &turso::Connection,
    context: &PrepareContext<'_>,
) -> Result<ExecutionProofProgress, HostError> {
    let (proof, started) = match load_ready_state(connection, context).await? {
        PrepareState::Progress(progress) => return Ok(progress),
        PrepareState::Ready { proof, started } => (proof, started),
    };
    let prepared = decision::build_decision(connection, context, &proof, &started).await?;
    decision::store_decision(connection, context, &proof, &started, &prepared).await?;
    Ok(ExecutionProofProgress::DecisionPrepared)
}

async fn load_ready_state(
    connection: &turso::Connection,
    context: &PrepareContext<'_>,
) -> Result<PrepareState, HostError> {
    let Some(proof) = load_proof(connection, context.operation_id, &context.connection_id).await?
    else {
        return Err(HostError::Identity);
    };
    let Some(stored_source_json) = proof.source_json.as_deref() else {
        return Ok(PrepareState::Progress(
            ExecutionProofProgress::WaitingForSource,
        ));
    };
    let Some(stored_source_digest) = proof.source_digest.as_deref() else {
        return Err(HostError::Journal);
    };
    if stored_source_json.len() > MAX_PROOF_BYTES {
        return Err(HostError::Journal);
    }
    let persisted: PersistedExecutionSource =
        serde_json::from_str(stored_source_json).map_err(|_| HostError::Journal)?;
    let persisted_bytes = persisted.canonical_json()?;
    if persisted_bytes != stored_source_json.as_bytes()
        || digest(&persisted_bytes) != stored_source_digest
        || stored_source_json != context.source_json
        || stored_source_digest != context.source_digest
        || persisted.validate().is_err()
        || !context.source.matches_challenge(&proof.challenge_document)
    {
        return Err(HostError::Journal);
    }
    let Some(started) =
        started::load(connection, context.operation_id, &context.connection_id).await?
    else {
        return Ok(PrepareState::Progress(
            ExecutionProofProgress::WaitingForStarted,
        ));
    };
    let replay_key = DecisionReplayKey {
        operation_id: context.operation_id,
        reservation_id: context.reservation_id,
        connection_id: &context.connection_id,
        request_digest: &context.request_digest,
        run_id: context.source.request().run_id(),
        run_attempt: context.source.request().run_attempt(),
        job_key: context.source.request().job_key(),
        challenge: context.source.request().challenge(),
        actual_request_id: started.request_id,
        runner_id: proof.runner_id,
        generation: proof.generation,
        policy_digest: &proof.policy_digest,
    };
    if decision_exists(connection, &replay_key).await? {
        return Ok(PrepareState::Progress(
            ExecutionProofProgress::DecisionAlreadyPrepared,
        ));
    }
    if proof.proof_state != "pending" {
        return Err(HostError::Journal);
    }
    if proof.challenge_state != "published" {
        return Err(HostError::Identity);
    }
    Ok(PrepareState::Ready {
        proof: Box::new(proof),
        started,
    })
}

const MAX_DECISION_BYTES: usize = 8 * 1024;

struct DecisionReplayKey<'a> {
    operation_id: i64,
    reservation_id: i64,
    connection_id: &'a str,
    request_digest: &'a str,
    run_id: u64,
    run_attempt: u32,
    job_key: &'a str,
    challenge: &'a str,
    actual_request_id: i64,
    runner_id: i64,
    generation: i64,
    policy_digest: &'a str,
}

async fn decision_exists(
    connection: &turso::Connection,
    expected: &DecisionReplayKey<'_>,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT reservation_id, actual_request_id, challenge, generation, policy_digest, request_digest, allow, delivery_state, decision_digest, decision_json FROM connection_execution_decisions WHERE intent_id = ?1 AND connection_id = ?2",
            (expected.operation_id, expected.connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored_reservation_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let actual_request_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let challenge: String = row.get(2).map_err(|_| HostError::Journal)?;
    let generation: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    let policy_digest: String = row.get(4).map_err(|_| HostError::Journal)?;
    let request_digest: String = row.get(5).map_err(|_| HostError::Journal)?;
    let allow: i64 = row.get(6).map_err(|_| HostError::Journal)?;
    let state: String = row.get(7).map_err(|_| HostError::Journal)?;
    let expected_digest: String = row.get(8).map_err(|_| HostError::Journal)?;
    let json: String = row.get(9).map_err(|_| HostError::Journal)?;
    let decision: PersistedExecutionDecision =
        serde_json::from_str(&json).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || !matches!(
            state.as_str(),
            "prepared" | "requested" | "delivered" | "uncertain" | "abandoned"
        )
        || digest(json.as_bytes()) != expected_digest
        || decision.canonical_json()?.as_slice() != json.as_bytes()
        || decision.sha256()? != expected_digest
        || !decision.valid()
        || decision.intent_id() != expected.operation_id
        || stored_reservation_id != expected.reservation_id
        || decision.reservation_id() != stored_reservation_id
        || decision.actual_request_id() != actual_request_id
        || actual_request_id != expected.actual_request_id
        || decision.runner_id() != expected.runner_id
        || i64::try_from(decision.policy_generation()).ok() != Some(expected.generation)
        || decision.policy_digest() != expected.policy_digest
        || decision.run_id() != expected.run_id
        || decision.run_attempt() != expected.run_attempt
        || decision.job_key() != expected.job_key
        || decision.challenge() != challenge
        || challenge != expected.challenge
        || i64::try_from(decision.policy_generation()).ok() != Some(generation)
        || decision.policy_digest() != policy_digest
        || decision.request_digest() != request_digest
        || request_digest != expected.request_digest
        || i64::from(decision.allow()) != allow
    {
        return Err(HostError::Journal);
    }
    Ok(true)
}
