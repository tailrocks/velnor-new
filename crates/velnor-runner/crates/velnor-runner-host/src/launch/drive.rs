//! Launch call context. Tokens stay out of `Debug` and are zeroized on drop.

use std::fmt;

use zeroize::Zeroize;

use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

/// Call context. Tokens are redacted in `Debug` and zeroized on drop.
pub(crate) struct Drive {
    /// Scale-set id.
    pub(crate) set_id: i64,
    /// Queue path. No host.
    pub(crate) queue_path: String,
    /// Queue bearer.
    pub(crate) queue_token: String,
    /// Admin bearer.
    pub(crate) admin_token: String,
    /// Selected Docker engine identity, captured before a launch row is prepared.
    pub(crate) docker_engine_id: Option<String>,
    /// Repository owner. Empty skips runner-directory calls.
    pub(crate) owner: String,
    /// Repository name. Empty skips runner-directory calls.
    pub(crate) repo: String,
    /// REST credential for `api.github.com`. Not a queue token.
    pub(crate) pat: String,
}

/// Owner, repository, and REST credential for one launch.
#[derive(Clone, Copy)]
pub(crate) struct Rest<'a> {
    /// Repository owner.
    pub(crate) owner: &'a str,
    /// Repository name.
    pub(crate) repo: &'a str,
    /// PAT. Not logged.
    pub(crate) pat: &'a str,
    /// Validated runner and `DinD` limits for new worker pairs.
    pub(crate) resource_budget: Option<ResourceBudget>,
}

impl fmt::Debug for Drive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Drive")
            .field("set_id", &self.set_id)
            .field("queue_path", &self.queue_path)
            .field("queue_token", &"[redacted]")
            .field("admin_token", &"[redacted]")
            .field("docker_engine_id", &self.docker_engine_id)
            .field("owner", &self.owner)
            .field("repo", &self.repo)
            .field("pat", &"[redacted]")
            .finish()
    }
}

impl Drive {
    pub(crate) fn from_rest(
        set_id: i64,
        queue_path: String,
        queue_token: String,
        admin_token: String,
        rest: Rest<'_>,
    ) -> Self {
        Self {
            set_id,
            queue_path,
            queue_token,
            admin_token,
            docker_engine_id: None,
            owner: rest.owner.to_owned(),
            repo: rest.repo.to_owned(),
            pat: rest.pat.to_owned(),
        }
    }
}

impl Drop for Drive {
    fn drop(&mut self) {
        self.queue_token.zeroize();
        self.admin_token.zeroize();
        self.pat.zeroize();
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

    /// Use `https://api.github.com` for the runner directory.
    ///
    /// # Errors
    ///
    /// Returns [`EnsureError::Endpoint`] when the origin is not `https`.
    fn use_github_api(&mut self) -> Result<(), EnsureError>;
}
