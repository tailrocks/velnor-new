use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Read,

    Write,

    None,
}

impl PermissionLevel {
    #[must_use]
    pub const fn none() -> Self {
        Self::None
    }

    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    pub contents: PermissionLevel,

    pub pull_requests: PermissionLevel,

    pub id_token: PermissionLevel,

    pub actions: PermissionLevel,

    #[serde(
        default = "PermissionLevel::none",
        skip_serializing_if = "PermissionLevel::is_none"
    )]
    pub packages: PermissionLevel,
}

impl Permissions {
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
