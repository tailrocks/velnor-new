//! Typed sampler status and timestamped cached values.

use std::time::Instant;

use super::super::GuestResourceSample;

/// Why a sample is unavailable or stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuestSampleFailure {
    /// A Docker operation failed or returned incomplete data.
    Docker,
}

/// The current cached sample and its collection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuestSampleSnapshot {
    /// Parsed resource values. Unknown measurements remain `None`.
    pub(crate) sample: GuestResourceSample,
    /// Collection status or the reason no sample can be used.
    pub(crate) status: GuestSampleStatus,
    /// Monotonic time when collection began, if any.
    pub(crate) sampled_at: Option<Instant>,
}

/// Whether a cached result is usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuestSampleStatus {
    /// No sample has completed yet.
    Pending,
    /// The bounded probe completed and its typed values can be read.
    Available,
    /// Sampling failed for the recorded reason.
    Unavailable(GuestSampleFailure),
    /// The cached sample exceeded the caller's freshness limit.
    Stale,
}
