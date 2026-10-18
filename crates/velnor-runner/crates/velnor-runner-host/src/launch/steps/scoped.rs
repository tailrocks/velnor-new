//! One source-approved request and one charged physical runner.

use std::future::Future;

use sha2::{Digest, Sha256};
use velnor_runner_github::{Poll, Transport};

use crate::error::HostError;
use crate::journal::{AssignmentKey, ConnectionJournal};
use crate::launch::{Drive, Lane};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::{acknowledge, assignment, source_blocked};

mod acquire;
mod runner;
/// Reserve host capacity, acquire one sourced request, then start one runner.
pub(in crate::launch) async fn launch_scoped<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    permit: crate::launch::StartPermit,
    docker: &bollard::Docker,
    expected_engine: &crate::launch::SelectedEngineContext,
    images: &crate::launch::VerifiedWorkerImages,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: Transport + Lane,
    S: FnOnce(&str, &[u8], crate::launch::bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let (connection, scoped, state) = scoped_context(ctx)?;
    if !ctx.source_authorized {
        return Err(source_blocked(
            super::source::SourceBlock::ClosedWorldUnproven,
        ));
    }
    validate_scope_binding(connection, scoped, ctx.set_id).await?;
    let Some((batch, request_id)) = assignment(polled)? else {
        return Ok(None);
    };
    let _admission = crate::launch::lock_connection(state, connection)?;
    let request = scoped
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(super::map_journal)?;
    acquire::require_authorized(scoped, &request).await?;
    lane.on_admin()?;

    let physical = runner::reserve(scoped, permit).await?;
    match acquire::run(lane, &ctx.queue_token, scoped, &request).await? {
        acquire::AcquireDisposition::Acquired => {}
        acquire::AcquireDisposition::NoEffect => {
            acquire::release_unstarted(scoped, physical.reservation_id).await?;
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "acquire",
            });
        }
        acquire::AcquireDisposition::Rejected => {
            acquire::release_unstarted(scoped, physical.reservation_id).await?;
            return Err(EnsureError::Rejected);
        }
        acquire::AcquireDisposition::Uncertain => return Err(EnsureError::Uncertain),
    }
    let started = runner::start(
        lane,
        &ctx.admin_token,
        scoped,
        &physical,
        docker,
        expected_engine,
        images,
        start,
    )
    .await?;
    acknowledge(lane, ctx, batch)?;
    Ok(Some(started))
}

fn scoped_context(
    ctx: &Drive,
) -> Result<
    (
        &crate::config::RepositoryConnection,
        &ConnectionJournal,
        &std::path::Path,
    ),
    EnsureError,
> {
    match (
        ctx.connection.as_ref(),
        ctx.scoped_journal.as_ref(),
        ctx.admission_state.as_deref(),
    ) {
        (Some(connection), Some(scoped), Some(state)) => Ok((connection, scoped, state)),
        _ => Err(source_blocked(
            super::source::SourceBlock::ClosedWorldUnproven,
        )),
    }
}

async fn validate_scope_binding(
    connection: &crate::config::RepositoryConnection,
    scoped: &ConnectionJournal,
    set_id: i64,
) -> Result<(), EnsureError> {
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(super::map_journal)?;
    let digest = canonical_binding_digest(connection)?;
    if binding.connection_id() == &connection.connection_id
        && binding.connection_id() == scoped.connection_id()
        && binding.scale_set_id() == set_id
        && binding.binding_digest() == digest
    {
        Ok(())
    } else {
        Err(EnsureError::Uncertain)
    }
}

fn canonical_binding_digest(
    connection: &crate::config::RepositoryConnection,
) -> Result<String, EnsureError> {
    let identity = connection
        .canonical_identity_bytes()
        .map_err(|_| EnsureError::Uncertain)?;
    let digest = Sha256::digest(identity);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").map_err(|_| EnsureError::Uncertain)?;
    }
    Ok(output)
}
