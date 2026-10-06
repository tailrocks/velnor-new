//! Create and start one runner after journaling its launch intent.

use std::future::Future;

use velnor_runner_github::{EncodedJit, SessionError, Transport, jit, jit_request};

use crate::error::HostError;
use crate::journal::Journal;
use crate::listen::map_listen;
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::super::mint_origin::MintOrigin;
use super::super::{Drive, Lane};
use super::{acknowledge, hold, map_journal, mark_done};

/// Inputs for the JIT and worker effects after intent was recorded.
pub(super) struct Request<'a> {
    /// Current session tokens and scale-set identity.
    pub(super) ctx: &'a Drive,
    /// Queue batch to acknowledge after the worker starts, if any.
    pub(super) batch: Option<&'a velnor_runner_github::ParsedBatch>,
    /// Durable launch journal.
    pub(super) journal: &'a Journal,
    /// Existing intent row identifier.
    pub(super) id: i64,
    /// Runner name submitted to the JIT endpoint.
    pub(super) name: &'a str,
    /// Whether `AcquireJobs` already accepted this offered job.
    pub(super) origin: MintOrigin,
}

pub(super) async fn run<T, S, F>(
    lane: &mut T,
    request: Request<'_>,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: Transport + Lane,
    S: FnOnce(&str, &[u8], super::super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    if !request
        .journal
        .claim_launch_jit(request.id)
        .await
        .map_err(map_journal)?
    {
        return hold(request.journal, request.id, EnsureError::Uncertain).await;
    }
    run_claimed(lane, request, start).await
}

pub(super) async fn run_claimed<T, S, F>(
    lane: &mut T,
    request: Request<'_>,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: Transport + Lane,
    S: FnOnce(&str, &[u8], super::super::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let Request {
        ctx,
        batch,
        journal,
        id,
        name,
        origin,
    } = request;
    let encoded = match fetch_jit(lane, ctx, name) {
        Ok(encoded) => encoded,
        Err(error) => {
            return super::super::runner_dir::after_jit(
                lane, ctx, journal, id, name, origin, error,
            )
            .await;
        }
    };
    let bound = super::super::bind::Bind::new(journal, id);
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

pub(super) async fn fail_jit(
    journal: &Journal,
    id: i64,
    origin: MintOrigin,
    error: SessionError,
) -> Result<Option<Started>, EnsureError> {
    let mapped = map_listen(error);
    journal
        .finish(id, origin.error_outcome(error.certainty()))
        .await
        .map_err(map_journal)?;
    Err(mapped)
}

fn fetch_jit<T>(lane: &mut T, ctx: &Drive, name: &str) -> Result<EncodedJit, SessionError>
where
    T: Transport + ?Sized,
{
    let body = jit_request(name)?;
    jit(lane, ctx.set_id, &ctx.admin_token, &body).map(|result| result.encoded_jit_config)
}
