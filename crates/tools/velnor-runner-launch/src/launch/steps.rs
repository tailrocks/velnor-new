//! Acquire, JIT, and ack for one offered id. The journal commits first.

use std::future::Future;

use velnor_runner_github::{
    Ack, AckScope, AcquireOutcome, Certainty, InnerKind, RefreshGate, SessionError, SessionRequest,
    ack, acquire,
};

use velnor_runner_host::HostError;
use velnor_runner_host::listen::map_listen;
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::worker::Started;
use velnor_runner_journal::journal::{Journal, Outcome};

mod mint;

use super::mint_origin::MintOrigin;
use super::{Drive, Lane};

mod admission;

pub(super) use admission::assignment;
pub(crate) use admission::{Idle, idle};

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
    let (id, fresh) = journal.begin_launch(&subject).await.map_err(map_journal)?;
    if docker_of(journal, id).await?.is_some() {
        return ack_bound(lane, ctx, batch, journal, id).await;
    }
    if !fresh {
        return hold(journal, id, EnsureError::Uncertain).await;
    }
    let name = generation_runner_name(id)?;
    let (requested_run, requested_job) = requested_identity(batch, request_id)?;
    journal
        .bind_launch_identity(
            id,
            Some(batch.message_id),
            Some(request_id),
            requested_run,
            requested_job.as_deref(),
            &name,
        )
        .await
        .map_err(map_journal)?;
    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(map_journal)?;
    match taken(lane, ctx, request_id) {
        Ok(AcquireOutcome::Acquired(ids)) if ids.is_empty() => reject_empty(journal, id).await,
        Ok(_) => {
            mint::run(
                lane,
                mint::Request {
                    ctx,
                    batch: Some(batch),
                    journal,
                    id,
                    name: &name,
                    origin: MintOrigin::AcquiredJob,
                },
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
    let subject = format!("m{}", batch.message_id);
    ensure_runner(lane, ctx, journal, &subject, Some(batch), start).await
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
    ensure_runner(lane, ctx, journal, name, None, start).await
}

fn taken<T>(lane: &mut T, ctx: &Drive, request_id: i64) -> Result<AcquireOutcome, SessionError>
where
    T: velnor_runner_github::Transport + Lane + ?Sized,
{
    let gate = RefreshGate::new();
    let refresh =
        |transport: &mut T, request: &mut SessionRequest| transport.refresh_queue(request, None);
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
    if docker_of(journal, id).await?.is_some() {
        return finish_live(lane, ctx, journal, id, batch).await;
    }
    if !fresh {
        return hold(journal, id, EnsureError::Uncertain).await;
    }
    let name = generation_runner_name(id)?;
    let message_id = batch.map(|event| event.message_id);
    journal
        .bind_launch_identity(id, message_id, None, None, None, &name)
        .await
        .map_err(map_journal)?;
    mint::run(
        lane,
        mint::Request {
            ctx,
            batch,
            journal,
            id,
            name: &name,
            origin: MintOrigin::AssignedPopulation,
        },
        start,
    )
    .await
}

fn requested_identity(
    batch: &velnor_runner_github::ParsedBatch,
    request_id: i64,
) -> Result<(Option<i64>, Option<String>), EnsureError> {
    let mut matching = batch.jobs.iter().filter(|event| {
        matches!(&event.kind, InnerKind::Available) && event.request_id == Some(request_id)
    });
    let event = matching.next().ok_or(EnsureError::Unexpected {
        status: 0,
        step: "queue identity",
    })?;
    if matching.next().is_some() {
        return Err(EnsureError::Unexpected {
            status: 0,
            step: "queue identity",
        });
    }
    Ok((event.workflow_run_id, event.job_id.clone()))
}

fn generation_runner_name(id: i64) -> Result<String, EnsureError> {
    if id <= 0 {
        return Err(EnsureError::Unexpected {
            status: 0,
            step: "runner identity",
        });
    }
    // The `g2_` namespace is distinct from legacy `v{request}`, `m{message}`,
    // and `s{session}` names. A migrated row with an unknown old name must not
    // match a new launch just because its row id equals an old request id.
    Ok(format!("g2_{id}"))
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
    let queue_path = lane.message_queue_path(&ctx.queue_path);
    let suffix = batch.message_id.to_string();
    let mut refresh = |transport: &mut T, request: &mut SessionRequest| {
        transport.refresh_queue(request, Some(&suffix))
    };
    let scope = AckScope {
        replay_safe: true,
        sole_unacquired_offer: false,
        queue_token: &ctx.queue_token,
    };
    let acked = ack(lane, &queue_path, batch, &scope, &gate, &mut refresh);
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
    if journal.read(id).await.map_err(map_journal)? == velnor_runner_host::IntentState::Done {
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
