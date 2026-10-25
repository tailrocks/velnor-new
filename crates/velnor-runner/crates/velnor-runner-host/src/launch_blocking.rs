//! Error type shared by the daemon controller entrypoint.

use crate::error::HostError;
use crate::scale_set::EnsureError;

/// The selected engine, journal, or controller failed to start or keep running.
#[derive(Debug, thiserror::Error)]
pub enum ListenFault {
    /// Docker, journal, or runtime setup failed. No token.
    #[error(transparent)]
    Host(#[from] HostError),
    /// Registration, acquire, or session failed. No token.
    #[error(transparent)]
    Ensure(#[from] EnsureError),
}
