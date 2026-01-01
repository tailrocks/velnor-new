//! One runner for the assigned population, then ack the batch.

use std::future::Future;

use crate::error::HostError;
use crate::journal::Journal;
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::super::mint_origin::MintOrigin;
use super::super::{Drive, Lane};
use super::mint;
use super::{acknowledge, docker_of, hold, map_journal, mark_done};

/// One runner for `statistics.totalAssignedJobs`, then ack `batch`.
pub(in crate::launch) async fn scale_id<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
    journal: &Journal,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: Fn(&str, &[u8], super::super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let name = format!("m{}", batch.message_id);
    let subject = name.clone();
    ensure_runner(lane, ctx, journal, &name, &subject, Some(batch), start).await
}

/// One runner from the create-session statistics. There is no message to ack.
pub(in crate::launch) async fn scale_unacked<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    name: &str,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: Fn(&str, &[u8], super::super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    // A shared Done "scale" row blocked later mints. The subject is this session.
    ensure_runner(lane, ctx, journal, name, name, None, start).await
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
    S: Fn(&str, &[u8], super::super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    lane.on_admin()?;
    let mut extra = 1u8;
    loop {
        let (id, fresh) = journal.begin_launch(subject).await.map_err(map_journal)?;
        if docker_of(journal, id).await?.is_some() {
            return finish_live(lane, ctx, journal, id, batch).await;
        }
        if !fresh && stale_cleared(lane, ctx, journal, id, name, &mut extra).await? {
            continue;
        }
        let minted = mint_attempt(
            lane,
            ctx,
            journal,
            MintAttempt {
                id,
                name,
                batch,
                fresh,
            },
            &start,
        )
        .await;
        match minted {
            Err(EnsureError::NameCleared) if extra > 0 => {
                extra -= 1;
            }
            Err(EnsureError::NameCleared) => return Err(EnsureError::NameSteady),
            other => return other,
        }
    }
}

/// Clear one stale empty row. True retries the mint on a fresh row.
async fn stale_cleared<T>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    id: i64,
    name: &str,
    extra: &mut u8,
) -> Result<bool, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
{
    use super::super::runner_dir::Decision;
    match super::super::runner_dir::release_empty(lane, ctx, journal, id, name).await? {
        // The cleared row was not a mint. The next mint keeps its one retry.
        Decision::Free if *extra > 0 => Ok(true),
        // The row is already failed. A repeat must not hold the slot.
        Decision::Free | Decision::Repeat => Err(EnsureError::NameSteady),
        Decision::Live | Decision::Unknown => Ok(false),
    }
}

/// Row and batch inputs for one mint attempt.
struct MintAttempt<'a> {
    id: i64,
    name: &'a str,
    batch: Option<&'a velnor_runner_github::ParsedBatch>,
    fresh: bool,
}

/// Mint once on row `id`, claiming the JIT or binding the identity first.
async fn mint_attempt<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    attempt: MintAttempt<'_>,
    start: &S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: Fn(&str, &[u8], super::super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let MintAttempt {
        id,
        name,
        batch,
        fresh,
    } = attempt;
    if !fresh {
        if !journal.claim_launch_jit(id).await.map_err(map_journal)? {
            return hold(journal, id, EnsureError::Uncertain).await;
        }
        return mint::run_claimed(
            lane,
            mint::Request {
                ctx,
                batch,
                journal,
                id,
                name,
                origin: MintOrigin::AssignedPopulation,
            },
            start,
        )
        .await;
    }
    if let Err(error) = journal
        .bind_launch_identity(id, ctx.set_id, None, name)
        .await
    {
        if journal
            .launch_identity_taken(ctx.set_id, name, id)
            .await
            .map_err(map_journal)?
        {
            return hold(journal, id, EnsureError::Uncertain).await;
        }
        return Err(map_journal(error));
    }
    mint::run(
        lane,
        mint::Request {
            ctx,
            batch,
            journal,
            id,
            name,
            origin: MintOrigin::AssignedPopulation,
        },
        start,
    )
    .await
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
