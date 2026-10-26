// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, release_jobs::ReleaseRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    None,
    Read,
    Write,
}

impl PermissionLevel {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobPermissions {
    pub contents: PermissionLevel,
    pub pull_requests: PermissionLevel,
    pub id_token: PermissionLevel,
}

impl JobPermissions {
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
