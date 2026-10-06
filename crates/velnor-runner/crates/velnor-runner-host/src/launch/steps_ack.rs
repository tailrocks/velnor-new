//! Queue acknowledgement follows a bound worker pair.

use velnor_runner_github::{Ack, AckScope, RefreshGate, ack};

use crate::journal::{Journal, Outcome};
use crate::listen::map_listen;
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::{Drive, Lane};

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
        Ok(Ack::Suppressed) => Err(unexpected_ack()),
        Err(error) => Err(map_listen(error)),
    };
    restored?;
    deleted
}

pub(super) async fn ack_bound<T>(
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

pub(super) async fn mark_done(journal: &Journal, id: i64) -> Result<(), EnsureError> {
    if journal.read(id).await.map_err(map_journal)? == crate::IntentState::Done {
        return Ok(());
    }
    journal.finish(id, Outcome::Done).await.map_err(map_journal)
}

fn map_journal(error: crate::error::HostError) -> EnsureError {
    match error {
        crate::error::HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}

fn unexpected_ack() -> EnsureError {
    EnsureError::Unexpected {
        status: 0,
        step: "ack",
    }
}
