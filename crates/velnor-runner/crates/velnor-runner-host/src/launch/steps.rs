//! Shared poll classification, source authorization, and ACK helpers.

use velnor_runner_github::{Ack, AckScope, Poll, RefreshGate, WireError, ack};

use crate::Offer;
use crate::error::HostError;
use crate::listen::map_listen;
use crate::offer;
use crate::scale_set::EnsureError;

mod offer_authorization;
mod scoped;
#[cfg(test)]
pub(crate) use scoped::{RunnerCreateFailure, runner_create_409, runner_create_409_with_jobs};
pub(crate) mod source;
mod source_authorization;

use super::{Drive, Lane};

pub(super) use offer_authorization::authorize_offer;
#[cfg(test)]
pub(super) use offer_authorization::authorize_offer_with_transport;
use source_authorization::source_blocked;

pub(crate) use scoped::{
    AcquiredProvisioning, CheckedAcquiredRecovery, KnownNoAcquire, ScopedProvisioningOutcome,
    acquire_authorized_request, acquire_scoped,
};

pub(super) fn assignment(
    polled: &Poll,
) -> Result<Option<(&velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(None);
    };
    match offer(polled) {
        Offer::Wait => Ok(None),
        Offer::Acquire { ids, .. } => one_request(batch, &ids),
    }
}

fn one_request<'a>(
    batch: &'a velnor_runner_github::ParsedBatch,
    ids: &[i64],
) -> Result<Option<(&'a velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    if ids.len() == 1 {
        Ok(Some((batch, ids[0])))
    } else {
        Err(EnsureError::Unexpected {
            status: 0,
            step: "capacity",
        })
    }
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
    let refresh = || Err(WireError::RefreshExhausted);
    let scope = AckScope {
        replay_safe: true,
        sole_unacquired_offer: false,
        queue_token: &ctx.queue_token,
    };
    let acked = ack(lane, &ctx.queue_path, batch, &scope, &gate, refresh);
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

pub(super) fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
