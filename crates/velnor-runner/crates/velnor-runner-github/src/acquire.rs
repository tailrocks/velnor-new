//! `AcquireJobs` result checks. Partial success is real. Unknown transport is not failure.

use std::collections::BTreeSet;

use crate::error::WireError;

/// What `acquirejobs` proved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireOutcome {
    /// Every returned id was requested. The list may be a subset.
    Acquired(Vec<i64>),
    /// The same id set was already acquired. Do not mint another grant.
    Noop,
}

/// Whether a failed call might still have happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Certainty {
    /// A timeout, reset, or unusable success response may hide an effect.
    Uncertain,
    /// The service explicitly rejected the call or local validation stopped it.
    Definite,
}

/// Transport failure class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFail {
    /// Deadline elapsed.
    Timeout,
    /// Connection reset.
    Reset,
    /// A completed HTTP status.
    Http(u16),
}

/// Map a transport failure to certainty. Timeouts and server failures stay occupied.
#[must_use]
pub const fn effect_certainty(fail: TransportFail) -> Certainty {
    match fail {
        TransportFail::Timeout | TransportFail::Reset | TransportFail::Http(500..=u16::MAX) => {
            Certainty::Uncertain
        }
        TransportFail::Http(_) => Certainty::Definite,
    }
}

/// Accept a partial value only when every returned id was requested.
///
/// # Errors
///
/// Returns [`WireError::OutsideRequest`] when the service returns a foreign id.
pub fn classify_acquire(
    requested: &[i64],
    returned: &[i64],
    already: &[i64],
) -> Result<AcquireOutcome, WireError> {
    let wanted: BTreeSet<i64> = requested.iter().copied().collect();
    if returned.iter().any(|id| !wanted.contains(id)) {
        return Err(WireError::OutsideRequest);
    }
    let got: BTreeSet<i64> = returned.iter().copied().collect();
    let prior: BTreeSet<i64> = already.iter().copied().collect();
    if !got.is_empty() && got == prior {
        return Ok(AcquireOutcome::Noop);
    }
    Ok(AcquireOutcome::Acquired(returned.to_vec()))
}
