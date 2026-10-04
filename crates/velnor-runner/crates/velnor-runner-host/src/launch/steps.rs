//! Acquire, JIT, and ack for one offered id. The journal commits first.

use velnor_runner_github::{
    JitResult, Poll, RunnerReference, SessionError, jit, jit_request, may_ack,
};

use crate::Offer;
use crate::error::HostError;
use crate::journal::{Journal, LaunchIdentity, LaunchReservation, Outcome};
use crate::listen::map_listen;
use crate::offer;
use crate::scale_set::EnsureError;
use crate::worker::{PreparedDind, Started};

use super::steps_ack::{acknowledge, finish_live, hold, mark_done};
use super::{Drive, Lane};

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

/// Classify one poll. One available id is acquired. An assigned population
/// starts one runner before ack. Anything else that is safe to delete is acked.
#[must_use]
pub(crate) fn idle(polled: &Poll) -> Idle {
    match polled {
        Poll::Empty => Idle::Empty,
        Poll::Batch(batch) => match offer(polled) {
            Offer::Acquire { ids, .. } if ids.len() == 1 => Idle::Launch,
            Offer::Wait if needs_scale(batch) && may_ack(batch, true) => Idle::Scale,
            Offer::Wait if may_ack(batch, true) => Idle::Ack,
            Offer::Acquire { .. } | Offer::Wait => Idle::Blocked,
        },
    }
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

/// One runner for `statistics.totalAssignedJobs`, then ack `batch`.
pub(super) async fn scale_id<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
    journal: &Journal,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: std::future::Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: std::future::Future<Output = Result<Started, HostError>>,
{
    let name = format!("m{}", batch.message_id);
    ensure_runner(lane, ctx, journal, &name, Some(batch), prepare, start).await
}

/// One runner from the create-session statistics. There is no message to ack.
pub(super) async fn scale_unacked<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    subject: &str,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: std::future::Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: std::future::Future<Output = Result<Started, HostError>>,
{
    ensure_runner(lane, ctx, journal, subject, None, prepare, start).await
}

async fn ensure_runner<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    subject: &str,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: std::future::Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: std::future::Future<Output = Result<Started, HostError>>,
{
    lane.on_admin()?;
    let reservation = journal
        .reserve_launch(subject, super::capacity::job_capacity())
        .await
        .map_err(map_journal)?;
    let id = match reservation {
        LaunchReservation::AtCapacity => return Ok(None),
        LaunchReservation::Existing(id) => {
            if docker_pair_bound(journal, id).await? {
                return finish_live(lane, ctx, journal, id, batch).await;
            }
            if journal.intent(id).await.map_err(map_journal)?.jit_requested {
                return Err(EnsureError::Uncertain);
            }
            id
        }
        LaunchReservation::New(id) => id,
    };
    mint(lane, ctx, batch, journal, id, false, prepare, start).await
}

pub(super) async fn mint<T, P, PF, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    journal: &Journal,
    id: i64,
    acquired: bool,
    prepare: P,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    P: FnOnce(LaunchIdentity) -> PF,
    PF: std::future::Future<Output = Result<PreparedDind, HostError>>,
    S: FnOnce(LaunchIdentity, PreparedDind, Vec<u8>) -> F,
    F: std::future::Future<Output = Result<Started, HostError>>,
{
    let identity = journal.launch_identity(id).await.map_err(map_journal)?;
    let name = format!("v{}", identity.launch_id());
    let prepared = match prepare(identity.clone()).await {
        Ok(prepared) => prepared,
        Err(HostError::PreparationFailedClean(_)) => {
            if acquired {
                return hold(journal, id, EnsureError::Uncertain).await;
            }
            journal
                .finish(id, Outcome::DefiniteFailure)
                .await
                .map_err(map_journal)?;
            journal.record_cleanup(id).await.map_err(map_journal)?;
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "dind-ready",
            });
        }
        Err(_) => return hold(journal, id, EnsureError::Uncertain).await,
    };
    journal
        .bind_dind_container(id, prepared.dind_id())
        .await
        .map_err(map_journal)?;
    if !journal.claim_jit(id).await.map_err(map_journal)? {
        return Err(EnsureError::Uncertain);
    }
    let jit_result = match fetch_jit(lane, ctx, &name) {
        Ok(result) => result,
        Err(error) => {
            let mapped = map_listen(error);
            return hold(journal, id, mapped).await;
        }
    };
    if !runner_matches(&jit_result.runner, ctx.set_id, &name) {
        return hold(
            journal,
            id,
            EnsureError::Unexpected {
                status: 0,
                step: "jit-runner",
            },
        )
        .await;
    }
    journal
        .bind_github_runner(id, &jit_result.runner.id.to_string())
        .await
        .map_err(map_journal)?;
    let Ok(started) = start(
        identity,
        prepared,
        jit_result.encoded_jit_config.expose().as_bytes().to_vec(),
    )
    .await
    else {
        return hold(journal, id, EnsureError::Uncertain).await;
    };
    journal
        .bind_pair(id, &started.runner_id, &started.dind_id)
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

fn runner_matches(runner: &RunnerReference, set_id: i64, name: &str) -> bool {
    runner.id > 0 && runner.runner_scale_set_id == set_id && runner.name == name
}

fn fetch_jit<T>(lane: &mut T, ctx: &Drive, name: &str) -> Result<JitResult, SessionError>
where
    T: velnor_runner_github::Transport + ?Sized,
{
    let body = jit_request(name)?;
    jit(lane, ctx.set_id, &ctx.admin_token, &body)
}

async fn docker_pair_bound(journal: &Journal, id: i64) -> Result<bool, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    Ok(rows
        .into_iter()
        .any(|row| row.id == id && row.docker_id.is_some() && row.dind_id.is_some()))
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
