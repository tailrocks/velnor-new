//! Acquire, JIT, and ack for one offered id. The journal commits first.

use std::future::Future;

use velnor_runner_github::{
    AcquireOutcome, Certainty, EncodedJit, Poll, RefreshGate, SessionError, acquire, jit,
    jit_request, may_ack,
};

use crate::Offer;
use crate::error::HostError;
use crate::journal::{Journal, LaunchIdentity, Outcome};
use crate::listen::map_listen;
use crate::offer;
use crate::reconcile::LaunchPhase;
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::{Drive, Lane};

mod acknowledge;
pub(super) use acknowledge::acknowledge;

/// What one poll allows before acquire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Idle {
    /// HTTP 202. The queue has nothing else.
    Empty,
    /// One `JobAvailable` id. Do not acknowledge yet.
    Launch,
    /// Assigned population is positive. Mint one JIT runner, then acknowledge.
    Scale,
    /// No offer and no assigned job. Delete the message so the next one can arrive.
    Ack,
    /// A message that must stay on the queue.
    Blocked,
}

/// Classify one poll. One available id is acquired. A positive current assigned
/// population starts one runner before ack. Missing or invalid census stays queued.
#[must_use]
pub(crate) fn idle(polled: &Poll) -> Idle {
    match polled {
        Poll::Empty => Idle::Empty,
        Poll::Batch(batch) => match offer(polled) {
            Offer::Acquire { ids, .. } if ids.len() == 1 => Idle::Launch,
            Offer::Wait if may_ack(batch, true) => match assigned_population(batch) {
                Some(population) if population > 0 => Idle::Scale,
                Some(0) => Idle::Ack,
                Some(_) | None => Idle::Blocked,
            },
            Offer::Acquire { .. } | Offer::Wait => Idle::Blocked,
        },
    }
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
    S: FnOnce(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let subject = format!("m{}r{request_id}", batch.message_id);
    let name = format!("v{request_id}");
    let identity = launch_identity(ctx, Some(request_id), &name)?;
    lane.on_admin()?;
    let (id, fresh) = journal
        .begin_prepared_launch(&subject, &identity)
        .await
        .map_err(map_journal)?;
    if docker_of(journal, id).await?.is_some() {
        return ack_bound(lane, ctx, batch, journal, id).await;
    }
    if !fresh {
        return hold(journal, id, EnsureError::Uncertain).await;
    }
    journal
        .advance_launch_phase(id, LaunchPhase::AcquireRequested)
        .await
        .map_err(map_journal)?;
    match taken(lane, ctx, request_id) {
        Ok(AcquireOutcome::Acquired(ids)) if ids.is_empty() => {
            journal
                .advance_launch_phase(id, LaunchPhase::Acquired)
                .await
                .map_err(map_journal)?;
            reject_empty(journal, id).await
        }
        Ok(AcquireOutcome::Acquired(_)) | Ok(AcquireOutcome::Noop) => {
            journal
                .advance_launch_phase(id, LaunchPhase::Acquired)
                .await
                .map_err(map_journal)?;
            mint(
                lane,
                ctx,
                Some(batch),
                journal,
                id,
                &identity.worker_volume,
                &name,
                start,
            )
            .await
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
    S: FnOnce(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let name = format!("m{}", batch.message_id);
    let subject = name.clone();
    ensure_runner(lane, ctx, journal, &name, &subject, Some(batch), start).await
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
    S: FnOnce(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    // Subject is this session's runner name. A shared "scale" row stayed Done
    // and blocked every later statistics mint, so an assigned job never got a runner.
    ensure_runner(lane, ctx, journal, name, name, None, start).await
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

async fn ensure_runner<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    name: &str,
    subject: &str,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let identity = launch_identity(ctx, None, name)?;
    lane.on_admin()?;
    let (id, fresh) = journal
        .begin_prepared_launch(subject, &identity)
        .await
        .map_err(map_journal)?;
    if docker_of(journal, id).await?.is_some() {
        return finish_live(lane, ctx, journal, id, batch).await;
    }
    if !fresh {
        return hold(journal, id, EnsureError::Uncertain).await;
    }
    mint(
        lane,
        ctx,
        batch,
        journal,
        id,
        &identity.worker_volume,
        name,
        start,
    )
    .await
}

async fn mint<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    journal: &Journal,
    id: i64,
    volume: &str,
    name: &str,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    journal
        .advance_launch_phase(id, LaunchPhase::JitRequested)
        .await
        .map_err(map_journal)?;
    let encoded = match fetch_jit(lane, ctx, name) {
        Ok(encoded) => encoded,
        Err(error) => {
            let mapped = map_listen(error);
            if matches!(mapped, EnsureError::Conflict) {
                journal
                    .finish(id, Outcome::DefiniteFailure)
                    .await
                    .map_err(map_journal)?;
                return Err(mapped);
            }
            return hold(journal, id, mapped).await;
        }
    };
    journal
        .advance_launch_phase(id, LaunchPhase::JitReceived)
        .await
        .map_err(map_journal)?;
    let bound = super::bind::Bind::new(journal, id);
    let Ok(started) = start(volume, encoded.expose().as_bytes(), bound).await else {
        return hold(journal, id, EnsureError::Uncertain).await;
    };
    journal
        .bind_worker(id, Some(&started.runner_id), Some(&started.dind_id))
        .await
        .map_err(map_journal)?;
    journal
        .advance_launch_phase(id, LaunchPhase::WorkerReady)
        .await
        .map_err(map_journal)?;
    if let Some(batch) = batch
    {
        journal
            .advance_launch_phase(id, LaunchPhase::AcknowledgementRequested)
            .await
            .map_err(map_journal)?;
        if let Err(error) = acknowledge(lane, ctx, batch) {
            return hold(journal, id, error).await;
        }
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

async fn finish_live<T>(
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
    {
        advance_phase_if_known(journal, id, LaunchPhase::AcknowledgementRequested).await?;
        if let Err(error) = acknowledge(lane, ctx, batch) {
            return hold(journal, id, error).await;
        }
    }
    mark_done(journal, id).await?;
    // Already recorded. Counting it fills `started` and the session stops
    // before a later JobAvailable. A live container still occupies its slot.
    Ok(None)
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
    advance_phase_if_known(journal, id, LaunchPhase::AcknowledgementRequested).await?;
    if let Err(error) = acknowledge(lane, ctx, batch) {
        return hold(journal, id, error).await;
    }
    mark_done(journal, id).await?;
    Ok(None)
}

async fn hold(
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
    advance_phase_if_known(journal, id, LaunchPhase::Complete).await?;
    if journal.read(id).await.map_err(map_journal)? == crate::IntentState::Done {
        return Ok(());
    }
    journal.finish(id, Outcome::Done).await.map_err(map_journal)
}

async fn docker_of(journal: &Journal, id: i64) -> Result<Option<String>, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    Ok(rows
        .into_iter()
        .find(|row| row.id == id)
        .and_then(|row| row.docker_id))
}

fn launch_identity(
    ctx: &Drive,
    request_id: Option<i64>,
    runner_name: &str,
) -> Result<LaunchIdentity, EnsureError> {
    let docker_engine_id = ctx
        .docker_engine_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or(EnsureError::Unexpected {
            status: 0,
            step: "docker identity",
        })?;
    let worker_volume = crate::worker::new_worker_volume().map_err(|_| {
        EnsureError::Unexpected {
            status: 0,
            step: "worker identity",
        }
    })?;
    if ctx.set_id <= 0 || runner_name.is_empty() {
        return Err(EnsureError::Unexpected {
            status: 0,
            step: "worker identity",
        });
    }
    Ok(LaunchIdentity {
        scale_set_id: ctx.set_id,
        request_id,
        runner_name: runner_name.to_owned(),
        worker_volume,
        docker_engine_id,
    })
}

async fn advance_phase_if_known(
    journal: &Journal,
    id: i64,
    phase: LaunchPhase,
) -> Result<(), EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let row = rows
        .iter()
        .find(|row| row.id == id)
        .ok_or(EnsureError::Unexpected {
            status: 0,
            step: "journal",
        })?;
    if row.launch_phase.is_some() {
        journal
            .advance_launch_phase(id, phase)
            .await
            .map_err(map_journal)?;
    }
    Ok(())
}

fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
