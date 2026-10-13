//! Acquire, JIT, and ack for one offered id. The journal commits first.

use std::future::Future;

use velnor_runner_github::{
    Ack, AckScope, AcquireOutcome, Certainty, EncodedJit, RefreshGate, SessionError, ack, acquire,
    jit, jit_request,
};

use crate::error::HostError;
use crate::journal::{Journal, Outcome};
use crate::listen::map_listen;
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::{Drive, Lane};

mod offer;
pub(super) use offer::assignment;
pub(crate) use offer::{Idle, idle};

#[cfg(test)]
#[path = "steps/assigned_resume_tests.rs"]
mod assigned_resume_tests;

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
        if journal.claim_launch_jit(id).await.map_err(map_journal)? {
            return mint_claimed(lane, ctx, Some(batch), journal, id, &name, start).await;
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
            mint(lane, ctx, Some(batch), journal, id, &name, start).await
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
    lane.on_admin()?;
    let (id, fresh) = journal.begin_launch(subject).await.map_err(map_journal)?;
    journal
        .bind_launch_identity(id, ctx.set_id, None, name)
        .await
        .map_err(map_journal)?;
    if docker_of(journal, id).await?.is_some() {
        return finish_live(lane, ctx, journal, id, batch).await;
    }
    if !fresh {
        if !journal.claim_launch_jit(id).await.map_err(map_journal)? {
            return hold(journal, id, EnsureError::Uncertain).await;
        }
        return mint_claimed(lane, ctx, batch, journal, id, name, start).await;
    }
    mint(lane, ctx, batch, journal, id, name, start).await
}

async fn mint<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    journal: &Journal,
    id: i64,
    name: &str,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    if !journal.claim_launch_jit(id).await.map_err(map_journal)? {
        return hold(journal, id, EnsureError::Uncertain).await;
    }
    mint_claimed(lane, ctx, batch, journal, id, name, start).await
}

async fn mint_claimed<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    journal: &Journal,
    id: i64,
    name: &str,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
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

fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
