use crate::journal::ConnectionJournal;
use crate::scale_set::EnsureError;
use velnor_runner_github::Poll;

use super::{Drive, Idle, assignment, authorize_offer, idle, source};

pub(super) fn source_blocked(block: source::SourceBlock) -> EnsureError {
    eprintln!("source_admission=blocked reason={}", block.code());
    EnsureError::SourceBlocked {
        reason: block.code(),
    }
}

/// Do a check of source policy before a guest probe, acquire call, or JIT request.
pub(in crate::launch) async fn authorize_poll(
    ctx: &mut Drive,
    polled: &Poll,
) -> Result<(), EnsureError> {
    if ctx.source_authorized {
        return Ok(());
    }
    authorize_source_poll(
        ctx.connection.as_ref(),
        ctx.scoped_journal.as_ref(),
        &ctx.pat,
        polled,
    )
    .await?;
    ctx.source_authorized = true;
    Ok(())
}

pub(in crate::launch) async fn authorize_poll_rest(
    rest: &crate::launch::Rest<'_>,
    polled: &Poll,
) -> Result<(), EnsureError> {
    if rest.source_authorized {
        return Ok(());
    }
    authorize_source_poll(rest.connection, rest.scoped_journal, rest.pat, polled).await
}

async fn authorize_source_poll(
    connection: Option<&crate::config::RepositoryConnection>,
    scoped: Option<&ConnectionJournal>,
    token: &str,
    polled: &Poll,
) -> Result<(), EnsureError> {
    if connection.is_none() {
        if scoped.is_some() {
            return Err(source_blocked(source::SourceBlock::Missing));
        }
        return Err(source_blocked(source::SourceBlock::ClosedWorldUnproven));
    }
    match idle(polled) {
        Idle::Launch => {
            let Some((batch, request_id)) = assignment(polled)? else {
                return Err(source_blocked(source::SourceBlock::Missing));
            };
            let _authorized = authorize_offer(connection, scoped, token, batch, request_id).await?;
        }
        Idle::Scale => {
            let Poll::Batch(batch) = polled else {
                return Err(source_blocked(source::SourceBlock::Missing));
            };
            authorize_population_message(scoped.is_some(), connection, token, batch)?;
        }
        Idle::Empty | Idle::Ack | Idle::Blocked => {}
    }
    Ok(())
}

pub(in crate::launch) fn authorize_population_message(
    scoped: bool,
    connection: Option<&crate::config::RepositoryConnection>,
    _token: &str,
    _batch: &velnor_runner_github::ParsedBatch,
) -> Result<(), EnsureError> {
    if scoped {
        return Err(source_blocked(source::SourceBlock::Missing));
    }
    if connection.is_none() {
        return Err(source_blocked(source::SourceBlock::Missing));
    }
    Err(source_blocked(source::SourceBlock::ClosedWorldUnproven))
}

#[cfg(test)]
mod source_gate_tests {
    use super::authorize_population_message;
    use crate::scale_set::EnsureError;
    use velnor_runner_github::ParsedBatch;

    #[test]
    fn scoped_population_without_request_binding_cannot_authorize_a_start() {
        let batch = ParsedBatch {
            message_id: 7,
            raw_body: String::new(),
            statistics: Some(velnor_runner_github::Statistics {
                total_assigned_jobs: 2,
                total_running_jobs: 0,
                total_registered_runners: 0,
                total_busy_runners: 0,
                total_idle_runners: 0,
                total_available_jobs: 0,
                total_acquired_jobs: 0,
            }),
            jobs: Vec::new(),
        };
        assert_eq!(
            authorize_population_message(true, None, "secret", &batch),
            Err(EnsureError::SourceBlocked {
                reason: "source_metadata_missing"
            })
        );
    }
}
