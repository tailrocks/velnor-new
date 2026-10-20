//! Typed GitHub token permission scopes for workflows and jobs.
use serde::{Deserialize, Serialize};
/// One GitHub token permission scope level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    /// Read-only access.
    Read,
    /// Read-write access.
    Write,
    /// No access.
    None,
}

impl PermissionLevel {
    /// Serde default for omitted scopes.
    #[must_use]
    pub const fn none() -> Self {
        Self::None
    }

    /// True for [`PermissionLevel::None`].
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Workflow or job permissions (typed scopes; renderer emits YAML).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    /// Repository contents scope.
    pub contents: PermissionLevel,
    /// Pull-requests scope.
    pub pull_requests: PermissionLevel,
    /// OIDC token scope (trusted publishing only).
    pub id_token: PermissionLevel,
    /// Actions scope.
    pub actions: PermissionLevel,
    /// Packages scope. Only a job whose Velnor-owned registry step reads a
    /// private npm package holds it (`read`); absent from serialized IR when none.
    #[serde(
        default = "PermissionLevel::none",
        skip_serializing_if = "PermissionLevel::is_none"
    )]
    pub packages: PermissionLevel,
}

impl Permissions {
    /// True when every scope is `write` (always rejected).
    #[must_use]
    pub fn is_write_all(&self) -> bool {
        [
            self.contents,
            self.pull_requests,
            self.id_token,
            self.actions,
        ]
        .iter()
        .all(|level| matches!(level, PermissionLevel::Write))
    }
}

impl Default for Permissions {
    /// CI default: `contents` read; every other scope none.
    fn default() -> Self {
        Self {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::None,
            packages: PermissionLevel::None,
        }
    }
}
