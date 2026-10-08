//! Queue and service credentials for one offer path.

use std::fmt;

use zeroize::Zeroize;

use velnor_runner_github::{SessionError, SessionRequest, WireError};
use velnor_runner_host::scale_set::EnsureError;

/// Call context. Tokens are redacted in [`Debug`] and zeroized on drop.
pub(crate) struct Drive {
    /// Scale-set id.
    pub(crate) set_id: i64,
    /// Queue path. No host.
    pub(crate) queue_path: String,
    /// Queue bearer.
    pub(crate) queue_token: String,
    /// Admin bearer.
    pub(crate) admin_token: String,
}

impl fmt::Debug for Drive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Drive")
            .field("set_id", &self.set_id)
            .field("queue_path", &self.queue_path)
            .field("queue_token", &"[redacted]")
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

impl Drop for Drive {
    fn drop(&mut self) {
        self.queue_token.zeroize();
        self.admin_token.zeroize();
    }
}

/// Switch the client between the admin origin and the message host.
pub(crate) trait Lane {
    /// Use the admin origin for acquire, JIT, and session delete.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError`] when the origin cannot be selected.
    fn on_admin(&mut self) -> Result<(), EnsureError>;

    /// Use the message-host origin for acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError`] when the origin cannot be selected.
    fn on_queue(&mut self) -> Result<(), EnsureError>;

    /// Current path after selecting the message queue origin.
    fn message_queue_path(&self, fallback: &str) -> String {
        fallback.to_owned()
    }

    /// Refresh the same session after one queue 401 and prepare the single replay.
    ///
    /// `ack_suffix` is present only for a message DELETE. Acquire replay stays on
    /// the admin origin and keeps its original request path and body.
    fn refresh_queue(
        &mut self,
        request: &mut SessionRequest,
        ack_suffix: Option<&str>,
    ) -> Result<(), SessionError> {
        let _ = (request, ack_suffix);
        Err(SessionError::Wire(WireError::Malformed))
    }
}
