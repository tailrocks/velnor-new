//! Release permission levels and the per-role least-privilege matrix.
//!
//! Typed levels only: raw permission strings never cross this boundary.
//! [`JobPermissions::expected`] fixes the exact matrix per
//! [`crate::release_jobs::ReleaseRole`]; [`JobPermissions::validate`]
//! enforces it plus the two structural rules (`id-token: write` needs a
//! pinned environment, validation roles never hold `contents: write`).

use crate::{RenderError, release_jobs::ReleaseRole};

/// One GitHub permission level (typed, never a raw string).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// No access.
    None,
    /// Read access.
    Read,
    /// Write access.
    Write,
}

impl PermissionLevel {
    /// YAML spelling of the level.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// Per-job least-privilege permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobPermissions {
    /// Repository contents access.
    pub contents: PermissionLevel,
    /// Pull-request access.
    pub pull_requests: PermissionLevel,
    /// OIDC token access.
    pub id_token: PermissionLevel,
}

impl JobPermissions {
    /// Exact least-privilege matrix for one role.
    #[must_use]
    pub fn expected(role: ReleaseRole) -> Self {
        use PermissionLevel::{None, Read, Write};
        match role {
            ReleaseRole::Preparation => Self {
                contents: Write,
                pull_requests: Write,
                id_token: None,
            },
            ReleaseRole::Preflight => Self {
                contents: Read,
                pull_requests: None,
                id_token: None,
            },
            ReleaseRole::PublishOidc => Self {
                contents: Write,
                pull_requests: Read,
                id_token: Write,
            },
            ReleaseRole::PublishBootstrap => Self {
                contents: Write,
                pull_requests: Read,
                id_token: None,
            },
            ReleaseRole::Reconcile => Self {
                contents: Read,
                pull_requests: Read,
                id_token: None,
            },
        }
    }

    /// Enforce the matrix plus the two structural permission rules.
    ///
    /// `id-token: write` requires a pinned job environment, and validation
    /// roles never hold `contents: write`.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidWorkflow`] for rule or matrix drift.
    pub fn validate(
        &self,
        role: ReleaseRole,
        environment: Option<&str>,
    ) -> Result<(), RenderError> {
        if self.id_token == PermissionLevel::Write && environment.is_none() {
            return Err(RenderError::InvalidWorkflow(
                "id_token_without_environment".to_owned(),
            ));
        }
        if role.is_validation() && self.contents == PermissionLevel::Write {
            return Err(RenderError::InvalidWorkflow(
                "contents_write_on_validation".to_owned(),
            ));
        }
        if *self != Self::expected(role) {
            return Err(RenderError::InvalidWorkflow(format!(
                "permission_matrix:{}",
                role.as_str()
            )));
        }
        Ok(())
    }
}
