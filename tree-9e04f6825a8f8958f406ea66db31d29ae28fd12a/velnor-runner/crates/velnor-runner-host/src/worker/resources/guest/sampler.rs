//! Snapshot types for guest samples consumed by the host capacity policy.

mod snapshot;

#[cfg(test)]
pub(crate) use snapshot::GuestSampleFailure;
pub(crate) use snapshot::{GuestSampleSnapshot, GuestSampleStatus};
