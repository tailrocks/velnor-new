//! Fixed outer-job headroom for native publication and recovery.

use crate::workflow::JobTimeout;

/// Default cap for preparation steps before a native publication operation.
pub const PUBLICATION_PREPARATION_STEP_DEFAULT_MINUTES: u16 = JobTimeout::DEFAULT_MINUTES;
/// The verified helper acquisition step keeps its existing explicit cap.
pub const RELEASE_ACQUIRE_STEP_TIMEOUT_MINUTES: u16 = 6;
/// Time reserved after the configured native operation for job cleanup.
pub const PUBLICATION_CLEANUP_HEADROOM_MINUTES: u16 = 1;

/// Largest native operation budget that fits the publisher's fixed job limit.
///
/// The publisher has four default-capped preparation steps and one six-minute
/// helper acquisition before the native operation. One additional minute is
/// reserved for completion and cleanup.
pub const MAX_PUBLICATION_RUNTIME_MINUTES: u16 = JobTimeout::MAX_MINUTES
    - (4 * PUBLICATION_PREPARATION_STEP_DEFAULT_MINUTES
        + RELEASE_ACQUIRE_STEP_TIMEOUT_MINUTES
        + PUBLICATION_CLEANUP_HEADROOM_MINUTES);
