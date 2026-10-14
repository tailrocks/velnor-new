//! Snapshot types for guest samples consumed by the host capacity policy.

mod snapshot;

#[cfg(test)]
#[expect(
    unused_imports,
    reason = "guest admission tests import the snapshot names"
)]
pub(crate) use snapshot::GuestSampleFailure;
#[cfg(test)]
#[expect(
    unused_imports,
    reason = "guest admission tests import the snapshot names"
)]
pub(crate) use snapshot::{GuestSampleSnapshot, GuestSampleStatus};
