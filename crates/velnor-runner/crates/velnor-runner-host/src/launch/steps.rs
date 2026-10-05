//! Acquire, JIT, and ack for one offered id. The journal commits first.

use std::future::Future;

use velnor_runner_github::{
    Ack, AckScope, AcquireOutcome, Certainty, EncodedJit, InnerKind, Poll, RefreshGate,
    SessionError, ack, acquire, jit, jit_request, may_ack,
};

use crate::Offer;
use crate::error::HostError;
use crate::journal::{Journal, Outcome};
use crate::listen::map_listen;
use crate::offer;
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::{Drive, Lane};

/// What one poll allows before acquire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Idle {
    /// HTTP 202. The queue has nothing else.
    Empty,
    /// One `JobAvailable` id. Do not acknowledge yet.
    Launch,
    /// Positive assigned population that is not `JobAssigned`.
    Scale,
    /// `JobAssigned` with no `JobStarted`. A started replay is [`Idle::Ack`].
    Mint,
    /// No offer and no assigned job. Delete the message so the next one can arrive.
    Ack,
    /// A message that must stay on the queue.
    Blocked,
}

/// Classify one poll. Acquire one id. Mint `JobAssigned`. Ack a started replay.
#[must_use]
pub(crate) fn idle(polled: &Poll) -> Idle {
    match polled {
        Poll::Empty => Idle::Empty,
        Poll::Batch(batch) => match offer(polled) {
            Offer::Acquire { ids, .. } if ids.len() == 1 => Idle::Launch,
            Offer::Wait
                if needs_scale(batch) && assigned_message(batch) && may_ack(batch, true) =>
            {
                if crate::assign::started_replay(batch) {
                    Idle::Ack
                } else {
                    Idle::Mint
                }
            }
            Offer::Wait if needs_scale(batch) && may_ack(batch, true) => Idle::Scale,
            Offer::Wait if may_ack(batch, true) => Idle::Ack,
            Offer::Acquire { .. } | Offer::Wait => Idle::Blocked,
        },
    }
}

/// Subject of a redelivered `JobAssigned`. Other polls have no exception.
#[must_use]
pub(super) fn mint_subject(polled: &Poll) -> Option<String> {
    let Poll::Batch(batch) = polled else {
        return None;
    };
    if idle(polled) == Idle::Mint {
        Some(format!("m{}", batch.message_id))
    } else {
        None
    }
}

fn assigned_message(batch: &velnor_runner_github::ParsedBatch) -> bool {
    batch
        .jobs
        .iter()
        .any(|job| matches!(job.kind, InnerKind::Assigned))
}

fn needs_scale(batch: &velnor_runner_github::ParsedBatch) -> bool {
    batch
        .statistics
        .as_ref()
        .is_some_and(|stats| stats.assigned_population() > 0)
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
    let (id, fresh) = journal.begin_launch(&subject).await.map_err(map_journal)?;
    if docker_of(journal, id).await?.is_some() {
        return ack_bound(lane, ctx, batch, journal, id).await;
    }
    if !fresh {
        return hold(journal, id, EnsureError::Uncertain).await;
    }
    match taken(lane, ctx, request_id) {
        Ok(AcquireOutcome::Acquired(ids)) if ids.is_empty() => reject_empty(journal, id).await,
        Ok(_) => {
            let name = format!("v{request_id}");
            mint(lane, ctx, Some(batch), journal, id, &name, &start).await
        }
        Err(error) => fail_acquire(journal, id, error).await,
    }
}

/// One runner for `statistics.totalAssignedJobs`, then ack `batch`.
pub(super) async fn scale_id<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
    journal: &Journal,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: Fn(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let name = format!("m{}", batch.message_id);
    let subject = name.clone();
    super::runner_dir::ensure_runner(lane, ctx, journal, &name, &subject, Some(batch), start).await
}

/// One runner from the create-session statistics. There is no message to ack.
pub(super) async fn scale_unacked<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    name: &str,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: Fn(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    // A shared Done "scale" row blocked later mints. The subject is this session.
    super::runner_dir::ensure_runner(lane, ctx, journal, name, name, None, start).await
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
    let outcome = match error.certainty() {
        Certainty::Uncertain => Outcome::Uncertain,
        Certainty::Definite => Outcome::DefiniteFailure,
    };
    journal.finish(id, outcome).await.map_err(map_journal)?;
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

pub(super) async fn mint<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    journal: &Journal,
    id: i64,
    name: &str,
    start: &S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: Fn(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let encoded = match fetch_jit(lane, ctx, name) {
        Ok(encoded) => encoded,
        Err(error) => {
            return super::runner_dir::after_jit(lane, ctx, journal, id, name, map_listen(error))
                .await;
        }
    };
    let bound = super::bind::Bind::new(journal, id);
    let Ok(volume) = crate::worker::new_worker_volume() else {
        return hold(
            journal,
            id,
            EnsureError::Unexpected {
                status: 0,
                step: "worker identity",
            },
        )
        .await;
    };
    let Ok(started) = start(&volume, encoded.expose().as_bytes(), bound).await else {
        return hold(journal, id, EnsureError::Uncertain).await;
    };
    journal
        .bind_worker(id, Some(&started.runner_id), Some(&started.dind_id))
        .await
        .map_err(map_journal)?;
    if let Some(batch) = batch
        && let Err(error) = acknowledge(lane, ctx, batch)
    {
        return hold(journal, id, error).await;
    }
    mark_done(journal, id).await?;
    Ok(Some(started))
}

fn fetch_jit<T>(lane: &mut T, ctx: &Drive, name: &str) -> Result<EncodedJit, SessionError>
where
    T: velnor_runner_github::Transport + ?Sized,
{
    let body = jit_request(name)?;
    jit(lane, ctx.set_id, &ctx.admin_token, &body)
}

pub(super) async fn finish_live<T>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    id: i64,
    batch: Option<&velnor_runner_github::ParsedBatch>,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
{
    if let Some(batch) = batch
        && let Err(error) = acknowledge(lane, ctx, batch)
    {
        return hold(journal, id, error).await;
    }
    mark_done(journal, id).await?;
    // Already recorded. Counting it fills `started` and the session stops
    // before a later JobAvailable. A live container still occupies its slot.
    Ok(None)
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
