use std::future::Future;

use velnor_runner_github::{InnerKind, Poll};

use crate::config::RepositoryConnection;
use crate::journal::{AssignmentKey, ConnectionJournal, SourceAuthorizationResult};
use crate::launch::steps::KnownNoAcquire;
use crate::launch::steps::source::AuthorizedSourceReceipt;
use crate::scale_set::EnsureError;

use super::deferred_offer::{DeferredAuthorizedOffer, DeferredAuthorizedOffers};

pub(crate) enum PositiveOfferAuthorization {
    Deferred(DeferredAuthorizedOffer),
    TerminalNoAcquire(KnownNoAcquire),
}

/// Recheck a source receipt before replaying its terminal no-acquire result.
pub(crate) async fn terminal_no_acquire_from_receipt(
    connection: &RepositoryConnection,
    scoped: &ConnectionJournal,
    set_id: i64,
    polled: &Poll,
    receipt: AuthorizedSourceReceipt,
) -> Result<Option<KnownNoAcquire>, EnsureError> {
    let Some((batch, request_id)) = crate::launch::steps::assignment(polled)? else {
        return Ok(None);
    };
    if receipt.connection_id() != &connection.connection_id
        || receipt.connection_id() != scoped.connection_id()
        || receipt.scale_set_id() != set_id
        || receipt.request_id() != request_id
        || receipt.message_id() != batch.message_id
    {
        return Err(EnsureError::Uncertain);
    }
    let identity = scoped
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|_| EnsureError::Uncertain)?;
    if scoped
        .record_source_authorization(&identity, &receipt)
        .await
        .map_err(|_| EnsureError::Uncertain)?
        != SourceAuthorizationResult::Replayed
    {
        return Err(EnsureError::Uncertain);
    }
    let Some((state, reservation_released)) = scoped
        .known_no_acquire_disposition(&identity)
        .await
        .map_err(|_| EnsureError::Uncertain)?
    else {
        return Ok(None);
    };
    let terminal = KnownNoAcquire::new(identity, state, reservation_released);
    if terminal.matches_poll(polled, scoped, set_id) {
        Ok(Some(terminal))
    } else {
        Err(EnsureError::Uncertain)
    }
}

/// Authorize a current queue offer or replay its exact terminal no-acquire result.
pub(crate) async fn authorize_positive_offer(
    connection: &RepositoryConnection,
    scoped: &ConnectionJournal,
    pat: &str,
    set_id: i64,
    polled: &Poll,
) -> Result<PositiveOfferAuthorization, EnsureError> {
    authorize_positive_offer_with(connection, scoped, pat, set_id, polled, || async {
        let Some((batch, request_id)) = crate::launch::steps::assignment(polled)? else {
            return Err(EnsureError::AdmissionClosed);
        };
        crate::launch::steps::authorize_offer(
            Some(connection),
            Some(scoped),
            pat,
            batch,
            request_id,
        )
        .await
    })
    .await
}

/// Check a terminal offer with the supplied source check.
pub(crate) async fn authorize_positive_offer_with<F, Fut>(
    connection: &RepositoryConnection,
    scoped: &ConnectionJournal,
    pat: &str,
    set_id: i64,
    polled: &Poll,
    authorize: F,
) -> Result<PositiveOfferAuthorization, EnsureError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Option<AuthorizedSourceReceipt>, EnsureError>>,
{
    let Some((_, request_id)) = crate::launch::steps::assignment(polled)? else {
        return Err(EnsureError::AdmissionClosed);
    };
    let identity = scoped
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|_| EnsureError::Uncertain)?;
    if scoped
        .known_no_acquire_disposition(&identity)
        .await
        .map_err(|_| EnsureError::Uncertain)?
        .is_some()
    {
        let receipt = authorize().await?.ok_or(EnsureError::Uncertain)?;
        let terminal =
            terminal_no_acquire_from_receipt(connection, scoped, set_id, polled, receipt)
                .await?
                .ok_or(EnsureError::Uncertain)?;
        return Ok(PositiveOfferAuthorization::TerminalNoAcquire(terminal));
    }
    let deferred = authorize_deferred_offer(connection, scoped, pat, set_id, polled).await?;
    Ok(PositiveOfferAuthorization::Deferred(deferred))
}

/// Persist exact source authorization for an offer that cannot be acquired now.
pub(crate) async fn authorize_deferred_offer(
    connection: &RepositoryConnection,
    scoped_journal: &ConnectionJournal,
    pat: &str,
    set_id: i64,
    polled: &Poll,
) -> Result<DeferredAuthorizedOffer, EnsureError> {
    let (batch, request_id) =
        crate::launch::steps::assignment(polled)?.ok_or(EnsureError::AdmissionClosed)?;
    if batch.message_id < 0 || request_id <= 0 {
        return Err(EnsureError::AdmissionClosed);
    }
    let source_receipt = crate::launch::steps::authorize_offer(
        Some(connection),
        Some(scoped_journal),
        pat,
        batch,
        request_id,
    )
    .await?
    .ok_or(EnsureError::Uncertain)?;
    DeferredAuthorizedOffer::from_poll_receipt(
        connection,
        scoped_journal,
        set_id,
        polled,
        request_id,
        source_receipt,
    )
    .await
}

/// Save source approval for each Available request when capacity is zero.
pub(crate) async fn authorize_deferred_offers(
    connection: &RepositoryConnection,
    scoped_journal: &ConnectionJournal,
    pat: &str,
    set_id: i64,
    polled: &Poll,
) -> Result<DeferredAuthorizedOffers, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return DeferredAuthorizedOffers::from_receipts(
            connection,
            scoped_journal,
            set_id,
            polled,
            Vec::new(),
        )
        .await;
    };
    if batch.jobs.len() > velnor_runner_github::MAX_POLL_MESSAGES {
        return Err(EnsureError::Uncertain);
    }
    let mut request_ids = std::collections::BTreeSet::new();
    for job in batch
        .jobs
        .iter()
        .filter(|job| job.kind == InnerKind::Available)
    {
        let Some(request_id) = job.request_id.filter(|request_id| *request_id > 0) else {
            return Err(EnsureError::Uncertain);
        };
        if !request_ids.insert(request_id) {
            return Err(EnsureError::Uncertain);
        }
    }
    let mut receipts = Vec::with_capacity(request_ids.len());
    for request_id in request_ids {
        let receipt = crate::launch::steps::authorize_offer(
            Some(connection),
            Some(scoped_journal),
            pat,
            batch,
            request_id,
        )
        .await?
        .ok_or(EnsureError::Uncertain)?;
        receipts.push((request_id, receipt));
    }
    DeferredAuthorizedOffers::from_receipts(connection, scoped_journal, set_id, polled, receipts)
        .await
}
