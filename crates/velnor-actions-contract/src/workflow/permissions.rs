//! Typed GitHub token permission scopes for workflows and jobs.
use serde::{Deserialize, Serialize};
/// One GitHub token permission scope level.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    /// Read-only access.
    Read,
    /// Read-write access.
    Write,
    /// No access.
    #[default]
    None,
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
    /// Issues scope (closed nightly observer only).
    #[serde(default)]
    pub issues: PermissionLevel,
    /// GitHub attestation writes require a closed approved publication role.
    #[serde(default)]
    pub attestations: PermissionLevel,
    /// Pages scope; writes require an authorized native deployment role.
    #[serde(default)]
    pub pages: PermissionLevel,
}

impl Permissions {
    /// True when all core token scopes are `write` (always rejected).
    #[must_use]
    pub fn is_write_all(&self) -> bool {
        [
            self.contents,
            self.pull_requests,
            self.id_token,
            self.actions,
            self.issues,
        ]
        .iter()
        .all(|level| matches!(level, PermissionLevel::Write))
    }
}

impl Default for Permissions {
    /// CI default: `contents`/`actions` read, everything else none.
    fn default() -> Self {
        Self {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::Read,
            issues: PermissionLevel::None,
            pages: PermissionLevel::None,
            attestations: PermissionLevel::None,
        }
    }
}
