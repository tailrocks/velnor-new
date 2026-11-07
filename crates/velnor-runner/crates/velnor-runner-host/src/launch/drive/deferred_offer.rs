// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_runner_github::{InnerKind, Poll};

use crate::{
    config::RepositoryConnection,
    journal::{AuthorizedOffer, ConnectionJournal},
    launch::steps::source::AuthorizedSourceReceipt,
    scale_set::EnsureError,
};

pub(crate) struct DeferredAuthorizedOffer {
    offer: AuthorizedOffer,
    envelope_message_id: i64,
}

impl DeferredAuthorizedOffer {
    pub(crate) fn offer(&self) -> &AuthorizedOffer {
        &self.offer
    }

    pub(crate) async fn from_poll_receipt(
        connection: &RepositoryConnection,
        scoped: &ConnectionJournal,
        set_id: i64,
        polled: &Poll,
        request_id: i64,
        receipt: AuthorizedSourceReceipt,
    ) -> Result<Self, EnsureError> {
        let Poll::Batch(batch) = polled else {
            return Err(EnsureError::AdmissionClosed);
        };
        let envelope_message_id = batch.message_id;
        if request_id <= 0
            || envelope_message_id < 0
            || !batch.jobs.iter().any(|job| {
                matches!(job.kind, InnerKind::Available | InnerKind::Assigned)
                    && job.request_id == Some(request_id)
            })
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

    pub(crate) async fn matches_current_envelope(
        &self,
        polled: &Poll,
        scoped: &ConnectionJournal,
        set_id: i64,
    ) -> bool {
        let Poll::Batch(batch) = polled else {
            return false;
        };
        let Ok(binding) = scoped.source_authorization_binding().await else {
            return false;
        };
        self.envelope_message_id == batch.message_id
            && batch.jobs.iter().any(|job| {
                matches!(job.kind, InnerKind::Available | InnerKind::Assigned)
                    && job.request_id == Some(self.offer.request_id())
            })
            && self.offer.connection_id() == scoped.connection_id()
            && self.offer.scale_set_id() == set_id
            && self.offer.binding_digest() == binding.binding_digest()
            && binding.scale_set_id() == set_id
    }
}

pub(crate) struct DeferredAuthorizedOffers {
    offers: Vec<DeferredAuthorizedOffer>,
}

impl DeferredAuthorizedOffers {
    pub(crate) fn empty() -> Self {
        Self { offers: Vec::new() }
    }

    pub(crate) async fn from_receipts(
        connection: &RepositoryConnection,
        scoped: &ConnectionJournal,
        set_id: i64,
        polled: &Poll,
        receipts: Vec<(i64, AuthorizedSourceReceipt)>,
    ) -> Result<Self, EnsureError> {
        let request_ids = available_request_ids(polled)?;
        if receipts.len() != request_ids.len() {
            return Err(EnsureError::Uncertain);
        }
        let mut seen = BTreeSet::new();
        let mut offers = Vec::with_capacity(receipts.len());
        for (request_id, receipt) in receipts {
            if !request_ids.contains(&request_id) || !seen.insert(request_id) {
                return Err(EnsureError::Uncertain);
            }
            offers.push(
                DeferredAuthorizedOffer::from_poll_receipt(
                    connection, scoped, set_id, polled, request_id, receipt,
                )
                .await?,
            );
        }
        if seen != request_ids {
            return Err(EnsureError::Uncertain);
        }
        Ok(Self { offers })
    }

    pub(crate) async fn matches_current_poll(
        &self,
        polled: &Poll,
        scoped: &ConnectionJournal,
        set_id: i64,
    ) -> bool {
        let Ok(request_ids) = available_request_ids(polled) else {
            return false;
        };
        let offer_ids = self
            .offers
            .iter()
            .map(|offer| offer.offer.request_id())
            .collect::<BTreeSet<_>>();
        if request_ids != offer_ids {
            return false;
        }
        for offer in &self.offers {
            if !offer.matches_current_envelope(polled, scoped, set_id).await {
                return false;
            }
        }
        true
    }
}

fn available_request_ids(polled: &Poll) -> Result<BTreeSet<i64>, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(BTreeSet::new());
    };
    if batch.jobs.len() > velnor_runner_github::MAX_POLL_MESSAGES || batch.message_id < 0 {
        return Err(EnsureError::Uncertain);
    }
    let mut ids = BTreeSet::new();
    for job in batch
        .jobs
        .iter()
        .filter(|job| matches!(job.kind, InnerKind::Available | InnerKind::Assigned))
    {
        let Some(request_id) = job.request_id.filter(|request_id| *request_id > 0) else {
            return Err(EnsureError::Uncertain);
        };
        if !ids.insert(request_id) {
            return Err(EnsureError::Uncertain);
        }
    }
    Ok(ids)
}
