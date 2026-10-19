use velnor_runner_github::Poll;

use crate::{
    config::RepositoryConnection,
    journal::{AuthorizedOffer, ConnectionJournal},
    launch::steps::source::AuthorizedSourceReceipt,
    scale_set::EnsureError,
};

/// Durable source authorization, bound to the current queue envelope.
pub(crate) struct DeferredAuthorizedOffer {
    offer: AuthorizedOffer,
    envelope_message_id: i64,
}

impl DeferredAuthorizedOffer {
    /// Original durable authorization; its message ID remains audit data.
    pub(crate) fn offer(&self) -> &AuthorizedOffer {
        &self.offer
    }

    /// Bind a fresh source receipt to its exact current queue envelope and durable offer.
    pub(crate) async fn from_poll_receipt(
        connection: &RepositoryConnection,
        scoped: &ConnectionJournal,
        set_id: i64,
        polled: &Poll,
        receipt: AuthorizedSourceReceipt,
    ) -> Result<Self, EnsureError> {
        let (batch, request_id) =
            crate::launch::steps::assignment(polled)?.ok_or(EnsureError::AdmissionClosed)?;
        let envelope_message_id = batch.message_id;
        if request_id <= 0
            || envelope_message_id < 0
            || receipt.connection_id() != &connection.connection_id
            || receipt.connection_id() != scoped.connection_id()
            || receipt.scale_set_id() != set_id
            || receipt.request_id() != request_id
            || receipt.message_id() != envelope_message_id
        {
            return Err(EnsureError::Uncertain);
        }
        let offer = scoped
            .authorized_offer_for_receipt(&receipt)
            .await
            .map_err(|_| EnsureError::Uncertain)?
            .ok_or(EnsureError::Uncertain)?;
        if offer.connection_id() != scoped.connection_id()
            || offer.scale_set_id() != set_id
            || offer.request_id() != request_id
            || offer.binding_digest() != receipt.binding_digest()
        {
            return Err(EnsureError::Uncertain);
        }
        Ok(Self {
            offer,
            envelope_message_id,
        })
    }

    /// Check current envelope identity and the live authorization binding.
    pub(crate) async fn matches_current_envelope(
        &self,
        polled: &Poll,
        scoped: &ConnectionJournal,
        set_id: i64,
    ) -> bool {
        let Ok(Some((batch, request_id))) = crate::launch::steps::assignment(polled) else {
            return false;
        };
        let Ok(binding) = scoped.source_authorization_binding().await else {
            return false;
        };
        self.envelope_message_id == batch.message_id
            && self.offer.connection_id() == scoped.connection_id()
            && self.offer.scale_set_id() == set_id
            && self.offer.request_id() == request_id
            && self.offer.binding_digest() == binding.binding_digest()
            && binding.scale_set_id() == set_id
    }
}

#[cfg(test)]
#[path = "deferred_offer/tests.rs"]
mod tests;
