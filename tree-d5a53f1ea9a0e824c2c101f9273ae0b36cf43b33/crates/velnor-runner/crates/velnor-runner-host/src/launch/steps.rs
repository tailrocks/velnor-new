//! Acquire, JIT, and ack for one offered id. The journal commits first.

use std::future::Future;

use velnor_runner_github::{
    Ack, AckScope, AcquireOutcome, Certainty, Poll, RefreshGate, SessionError, ack, acquire,
    may_ack,
};

use crate::Offer;
use crate::error::HostError;
use crate::journal::{Journal, Outcome};
use crate::listen::map_listen;
use crate::offer;
use crate::scale_set::EnsureError;
use crate::worker::Started;

mod mint;
mod scale;

use super::mint_origin::MintOrigin;
use super::{Drive, Lane};

pub(super) use scale::{scale_id, scale_unacked};

/// What one poll allows before acquire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Idle {
    /// HTTP 202. The queue has nothing else.
    Empty,
    /// One `JobAvailable` id. Do not acknowledge yet.
    Launch,
    /// Assigned population is positive. Mint one JIT runner, then acknowledge.
    Scale,
    /// The batch is safe to acknowledge, even when assigned population is unknown.
    /// Deleting it does not free a running slot.
    Ack,
    /// A message that must stay on the queue.
    Blocked,
}

/// Classify one poll. One available id is acquired. A positive current assigned
/// population starts one runner before ack. Missing census allows only empty or
/// start/completion-only batches to be acknowledged; it never implies zero jobs.
/// A batch whose every assigned job already started is acknowledged.
#[must_use]
pub(crate) fn idle(polled: &Poll) -> Idle {
    match polled {
        Poll::Empty => Idle::Empty,
        Poll::Quarantined(_) => Idle::Blocked,
        Poll::Batch(batch) => match offer(polled) {
            Offer::Acquire { ids, .. } if ids.len() == 1 => Idle::Launch,
            Offer::Wait if may_ack(batch, true) => {
                if absent_census_progress(batch)
                    || batch.statistics.is_some() && crate::assign::started_replay(batch)
                {
                    Idle::Ack
                } else {
                    match assigned_population(batch) {
                        Some(population) if population > 0 => Idle::Scale,
                        Some(0) => Idle::Ack,
                        Some(_) | None => Idle::Blocked,
                    }
                }
            }
            Offer::Acquire { .. } | Offer::Wait => Idle::Blocked,
        },
    }
}

/// Subject of a redelivered assigned poll. Other polls have no exception.
#[must_use]
pub(super) fn mint_subject(polled: &Poll) -> Option<String> {
    let Poll::Batch(batch) = polled else {
        return None;
    };
    if idle(polled) != Idle::Launch {
        return None;
    }
    let (_, request_id) = assignment(polled).ok().flatten()?;
    Some(format!("m{}r{request_id}", batch.message_id))
}

fn absent_census_progress(batch: &velnor_runner_github::ParsedBatch) -> bool {
    batch.statistics.is_none() && (batch.jobs.is_empty() || crate::assign::progress_only(batch))
}

fn assigned_population(batch: &velnor_runner_github::ParsedBatch) -> Option<i64> {
    batch
        .statistics
        .as_ref()
        .map(velnor_runner_github::Statistics::assigned_population)
        .filter(|population| *population >= 0)
}

pub(super) fn assignment(
    polled: &Poll,
) -> Result<Option<(&velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(None);
    };
    match offer(polled) {
        Offer::Wait => Ok(None),
        Offer::Acquire { ids, .. } => one_request(batch, &ids),
    }
}

fn one_request<'a>(
    batch: &'a velnor_runner_github::ParsedBatch,
    ids: &[i64],
) -> Result<Option<(&'a velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    if ids.len() == 1 {
        Ok(Some((batch, ids[0])))
    } else {
        Err(EnsureError::Unexpected {
            status: 0,
            step: "capacity",
        })
    }
}
#[cfg(test)]
#[path = "steps/assigned_resume_tests.rs"]
mod assigned_resume_tests;

fn acquired_request<'a>(
    ctx: &'a Drive,
    batch: &'a velnor_runner_github::ParsedBatch,
    journal: &'a Journal,
    id: i64,
    name: &'a str,
) -> mint::Request<'a> {
    mint::Request {
        ctx,
        batch: Some(batch),
        journal,
        id,
        name,
        origin: MintOrigin::AcquiredJob,
    }
}

pub(super) async fn launch_id<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
    journal: &Journal,
    request_id: i64,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: Fn(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    lane.on_admin()?;
    let subject = format!("m{}r{request_id}", batch.message_id);
    let name = format!("v{request_id}");
    let (id, fresh) = journal
        .begin_assigned_launch(&subject, ctx.set_id, request_id, &name)
        .await
        .map_err(map_journal)?;
    if docker_of(journal, id).await?.is_some() {
        return ack_bound(lane, ctx, batch, journal, id).await;
    }
    if !fresh {
        if super::runner_dir::cleared_repeat(journal, id).await? {
            return Err(EnsureError::NameCleared);
        }
        if journal.claim_launch_jit(id).await.map_err(map_journal)? {
            return mint::run_claimed(
                lane,
                acquired_request(ctx, batch, journal, id, &name),
                start,
            )
            .await;
        }
        if !journal
            .claim_assigned_acquire(id)
            .await
            .map_err(map_journal)?
        {
            return hold(journal, id, EnsureError::Uncertain).await;
        }
    } else if !journal
        .claim_assigned_acquire(id)
        .await
        .map_err(map_journal)?
    {
        return hold(journal, id, EnsureError::Uncertain).await;
    }
    match taken(lane, ctx, request_id) {
        Ok(AcquireOutcome::Acquired(ids)) if ids.as_slice() == [request_id] => {
            journal
                .record_assigned_acquire(id, true)
                .await
                .map_err(map_journal)?;
            mint::run(
                lane,
                acquired_request(ctx, batch, journal, id, &name),
                start,
            )
            .await
        }
        Ok(AcquireOutcome::Noop) => {
            journal
                .record_assigned_acquire(id, true)
                .await
                .map_err(map_journal)?;
            hold(journal, id, EnsureError::Uncertain).await
        }
        Ok(AcquireOutcome::Acquired(_)) => {
            journal
                .record_assigned_acquire(id, false)
                .await
                .map_err(map_journal)?;
            reject_empty(journal, id).await
        }
        Err(error) => fail_acquire(journal, id, error).await,
    }
}

fn taken<T>(lane: &mut T, ctx: &Drive, request_id: i64) -> Result<AcquireOutcome, SessionError>
where
    T: velnor_runner_github::Transport + ?Sized,
{
    let gate = RefreshGate::new();
    let refresh = || Ok(());
    acquire(
        lane,
        ctx.set_id,
        &[request_id],
        &[],
        &ctx.queue_token,
        &gate,
        refresh,
    )
}

async fn fail_acquire(
    journal: &Journal,
    id: i64,
    error: SessionError,
) -> Result<Option<Started>, EnsureError> {
    if error.certainty() == Certainty::Uncertain {
        journal
            .finish(id, Outcome::Uncertain)
            .await
            .map_err(map_journal)?;
    } else {
        journal
            .record_assigned_acquire(id, false)
            .await
            .map_err(map_journal)?;
    }
    Err(map_listen(error))
}

async fn reject_empty(journal: &Journal, id: i64) -> Result<Option<Started>, EnsureError> {
    journal
        .finish(id, Outcome::DefiniteFailure)
        .await
        .map_err(map_journal)?;
    Err(EnsureError::Unexpected {
        status: 0,
        step: "acquire",
    })
}

pub(super) fn acknowledge<T>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
) -> Result<(), EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
{
    lane.on_queue()?;
    let gate = RefreshGate::new();
    let refresh = || Ok(());
    let scope = AckScope {
        replay_safe: true,
        sole_unacquired_offer: false,
        queue_token: &ctx.queue_token,
    };
    let acked = ack(lane, &ctx.queue_path, batch, &scope, &gate, refresh);
    let restored = lane.on_admin();
    let deleted = match acked {
        Ok(Ack::Deleted) => Ok(()),
        Ok(Ack::Suppressed) => Err(EnsureError::Unexpected {
            status: 0,
            step: "ack",
        }),
        Err(error) => Err(map_listen(error)),
    };
    restored?;
    deleted
}

async fn ack_bound<T>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
    journal: &Journal,
    id: i64,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
{
    if let Err(error) = acknowledge(lane, ctx, batch) {
        return hold(journal, id, error).await;
    }
    mark_done(journal, id).await?;
    Ok(None)
}

pub(super) async fn hold(
    journal: &Journal,
    id: i64,
    error: EnsureError,
) -> Result<Option<Started>, EnsureError> {
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(map_journal)?;
    Err(error)
}

/// Settle one JIT error without the runner directory.
pub(super) async fn fail_jit(
    journal: &Journal,
    id: i64,
    origin: MintOrigin,
    error: SessionError,
) -> Result<Option<Started>, EnsureError> {
    mint::fail_jit(journal, id, origin, error).await
}

async fn mark_done(journal: &Journal, id: i64) -> Result<(), EnsureError> {
    if journal.read(id).await.map_err(map_journal)? == crate::IntentState::Done {
        return Ok(());
    }
    journal.finish(id, Outcome::Done).await.map_err(map_journal)
}

pub(super) async fn docker_of(journal: &Journal, id: i64) -> Result<Option<String>, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    Ok(rows
        .into_iter()
        .find(|row| row.id == id)
        .and_then(|row| row.docker_id))
}

pub(super) fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
