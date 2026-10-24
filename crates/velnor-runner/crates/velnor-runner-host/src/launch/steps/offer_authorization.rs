use super::{source, source_blocked};
use crate::journal::{AssignmentKey, ConnectionJournal};
use crate::scale_set::EnsureError;
use velnor_runner_github::Transport;

pub(in crate::launch) async fn authorize_offer(
    connection: Option<&crate::config::RepositoryConnection>,
    scoped: Option<&ConnectionJournal>,
    token: &str,
    batch: &velnor_runner_github::ParsedBatch,
    request_id: i64,
) -> Result<Option<source::AuthorizedSourceReceipt>, EnsureError> {
    let mut transport = crate::https::HttpsTransport::new(crate::listen::GITHUB_API)
        .map_err(|_| source_blocked(source::SourceBlock::ApiUnavailable))?;
    authorize_offer_with_transport(connection, scoped, token, batch, request_id, &mut transport)
        .await
}

pub(in crate::launch) async fn authorize_offer_with_transport<T: Transport + ?Sized>(
    connection: Option<&crate::config::RepositoryConnection>,
    scoped: Option<&ConnectionJournal>,
    token: &str,
    batch: &velnor_runner_github::ParsedBatch,
    request_id: i64,
    transport: &mut T,
) -> Result<Option<source::AuthorizedSourceReceipt>, EnsureError> {
    let Some(connection) = connection else {
        return if scoped.is_some() {
            Err(source_blocked(source::SourceBlock::Missing))
        } else {
            Err(source_blocked(source::SourceBlock::ClosedWorldUnproven))
        };
    };
    let source = batch
        .jobs
        .iter()
        .find(|job| {
            matches!(job.kind, velnor_runner_github::InnerKind::Available)
                && job.request_id == Some(request_id)
        })
        .map(|job| &job.source)
        .ok_or_else(|| source_blocked(source::SourceBlock::Missing))?;
    if source.complete().is_none() {
        return Err(source_blocked(source::SourceBlock::Missing));
    }
    let scoped = scoped.ok_or_else(|| source_blocked(source::SourceBlock::ClosedWorldUnproven))?;
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(|_| source_blocked(source::SourceBlock::ClosedWorldUnproven))?;
    let receipt = source::authorize_job_with_transport(
        connection,
        &binding,
        source,
        request_id,
        batch.message_id,
        token,
        transport,
    )
    .map_err(source_blocked)?;
    let identity = scoped
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|_| source_blocked(source::SourceBlock::ClosedWorldUnproven))?;
    scoped
        .record_source_authorization(&identity, &receipt)
        .await
        .map_err(|_| source_blocked(source::SourceBlock::ClosedWorldUnproven))?;
    Ok(Some(receipt))
}
