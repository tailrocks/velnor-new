//! Typed sampler status and timestamped cached values.

use std::time::Instant;

use super::super::GuestResourceSample;

/// Why a sample is unavailable or stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuestSampleFailure {
    /// The sampler has no Tokio runtime in which to run.
    RuntimeUnavailable,
    /// The sampler thread failed or panicked before joining cleanly.
    SamplerTask,
    /// The refresh interval is zero.
    InvalidInterval,
    /// The selected Docker daemon did not match the supplied engine identity.
    EngineIdentity,
    /// The local controller image tag was absent or did not resolve safely.
    ProbeImage,
    /// Docker guest information could not form a safe fixed probe request.
    ProbeInput,
    /// A container at the reserved name had a different owner identity.
    Ownership,
    /// A Docker operation failed or returned incomplete data.
    Docker,
    /// A bounded Docker operation timed out.
    Timeout,
    /// The probe could not be created or its create response was uncertain.
    ProbeCreate,
    /// The daemon did not settle an uncertain create within the attempt window.
    CreateUncertain,
    /// The probe did not exit successfully.
    ProbeExit,
    /// Probe output was missing or was not valid UTF-8.
    ProbeOutput,
    /// Probe output exceeded the fixed byte limit.
    OutputLimit,
    /// Owned probe removal could not be confirmed.
    Cleanup,
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

impl GuestSampleSnapshot {
    /// Build a typed result and record when collection began.
    pub(super) fn completed(
        result: Result<GuestResourceSample, GuestSampleFailure>,
        sampled_at: Instant,
    ) -> Self {
        let (sample, status) = match result {
            Ok(sample) => (sample, GuestSampleStatus::Available),
            Err(reason) => (
                GuestResourceSample::default(),
                GuestSampleStatus::Unavailable(reason),
            ),
        };
        Self {
            sample,
            status,
            sampled_at: Some(sampled_at),
        }
    }
}
