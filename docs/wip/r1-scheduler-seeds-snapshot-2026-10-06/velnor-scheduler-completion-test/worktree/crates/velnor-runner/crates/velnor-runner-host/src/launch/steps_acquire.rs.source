//! Durable assignment acquisition. Never repeat an uncertain AcquireJobs call.

use std::future::Future;

use velnor_runner_github::{AcquireOutcome, Certainty, RefreshGate, SessionError, acquire};

use crate::error::HostError;
use crate::journal::{Journal, LaunchIdentity, LaunchReservation, Outcome};
use crate::listen::map_listen;
use crate::scale_set::EnsureError;
use crate::worker::{PreparedDind, Started};

use super::steps::mint;
use super::steps_ack::acknowledge;
use super::steps_ack::{ack_bound, hold};
use super::{Drive, Lane, capacity};

pub(super) async fn launch_id<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
    journal: &Journal,
    request_id: i64,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    launch_reserved(lane, ctx, batch, journal, request_id, None, prepare, start).await
}

/// Resume a reservation already made by the poll admission transaction.
pub(super) async fn launch_reserved<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
    journal: &Journal,
    request_id: i64,
    reserved: Option<LaunchReservation>,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    lane.on_admin()?;
    let reservation = match reserved {
        Some(reservation) => reservation,
        None => journal
            .reserve_assignment(
                ctx.set_id,
                request_id,
                batch.message_id,
                capacity::job_capacity(),
            )
            .await
            .map_err(super::steps::map_journal)?,
    };
    let id = match reservation {
        LaunchReservation::AtCapacity => return Ok(None),
        LaunchReservation::Completed(_) => {
            acknowledge(lane, ctx, batch)?;
            return Ok(None);
        }
        LaunchReservation::Existing(id) => id,
        LaunchReservation::New(id) => id,
    };
    let row = journal
        .intent(id)
        .await
        .map_err(super::steps::map_journal)?;
    let assignment_key = format!("{}:{request_id}", ctx.set_id);
    if row.assignment_key.as_deref() != Some(assignment_key.as_str()) {
        return Err(EnsureError::Unexpected {
            status: 0,
            step: "assignment-reservation",
        });
    }
    if row.docker_id.is_some() && row.dind_id.is_some() {
        return ack_bound(lane, ctx, batch, journal, id).await;
    }
    if row.jit_requested {
        return Err(EnsureError::Uncertain);
    }
    if row.acquire_resolved && !row.acquired {
        return reject_empty(journal, id).await;
    }
    if !row.acquired && !acquire_request(lane, ctx, journal, id, request_id).await? {
        return reject_empty(journal, id).await;
    }
    mint(lane, ctx, Some(batch), journal, id, true, prepare, start).await
}

async fn acquire_request<T>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    id: i64,
    request_id: i64,
) -> Result<bool, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
{
    if !journal
        .claim_acquire(id)
        .await
        .map_err(super::steps::map_journal)?
    {
        return Err(EnsureError::Uncertain);
    }
    match taken(lane, ctx, request_id) {
        Ok(AcquireOutcome::Acquired(ids)) if ids == [request_id] => {
            journal
                .resolve_acquire(id, true)
                .await
                .map_err(super::steps::map_journal)?;
            Ok(true)
        }
        Ok(AcquireOutcome::Acquired(ids)) if ids.is_empty() => {
            journal
                .resolve_acquire(id, false)
                .await
                .map_err(super::steps::map_journal)?;
            Ok(false)
        }
        Ok(AcquireOutcome::Acquired(_) | AcquireOutcome::Noop) => {
            hold(journal, id, EnsureError::Uncertain).await?;
            Err(EnsureError::Uncertain)
        }
        Err(error) => fail_acquire(journal, id, error).await,
    }
}

fn taken<T>(lane: &mut T, ctx: &Drive, request_id: i64) -> Result<AcquireOutcome, SessionError>
where
    T: velnor_runner_github::Transport + ?Sized,
{
    let gate = RefreshGate::new();
    acquire(
        lane,
        ctx.set_id,
        &[request_id],
        &[],
        &ctx.queue_token,
        &gate,
        || Ok(()),
    )
}

async fn fail_acquire(
    journal: &Journal,
    id: i64,
    error: SessionError,
) -> Result<bool, EnsureError> {
    let outcome = match error.certainty() {
        Certainty::Uncertain => Outcome::Uncertain,
        Certainty::Definite => Outcome::DefiniteFailure,
    };
    if error.certainty() == Certainty::Definite {
        journal
            .resolve_acquire(id, false)
            .await
            .map_err(super::steps::map_journal)?;
    }
    journal
        .finish(id, outcome)
        .await
        .map_err(super::steps::map_journal)?;
    if error.certainty() == Certainty::Definite {
        journal
            .record_cleanup(id)
            .await
            .map_err(super::steps::map_journal)?;
    }
    Err(map_listen(error))
}

async fn reject_empty(journal: &Journal, id: i64) -> Result<Option<Started>, EnsureError> {
    journal
        .finish(id, Outcome::DefiniteFailure)
        .await
        .map_err(super::steps::map_journal)?;
    journal
        .record_cleanup(id)
        .await
        .map_err(super::steps::map_journal)?;
    Err(EnsureError::Unexpected {
        status: 0,
        step: "acquire",
    })
}
