// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use zeroize::Zeroize;

use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

pub(crate) struct Drive {
    pub(crate) set_id: i64,

    pub(crate) queue_path: String,

    pub(crate) queue_token: String,

    pub(crate) admin_token: String,

    pub(crate) docker_engine_id: Option<String>,

    pub(crate) owner: String,

    pub(crate) repo: String,

    pub(crate) pat: String,
}

#[derive(Clone, Copy)]
pub(crate) struct Rest<'a> {
    pub(crate) owner: &'a str,

    pub(crate) repo: &'a str,

    pub(crate) pat: &'a str,

    pub(crate) resource_budget: Option<ResourceBudget>,

    pub(crate) static_capacity: bool,

    pub(crate) guest_admission: GuestAdmission,
}

impl Rest<'_> {
    #[must_use]
    pub(crate) const fn permits_start(self) -> bool {
        self.static_capacity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuestAdmission {
    Unavailable,
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

pub(crate) trait Lane {
    fn on_admin(&mut self) -> Result<(), EnsureError>;

    fn on_queue(&mut self) -> Result<(), EnsureError>;

    fn use_github_api(&mut self) -> Result<(), EnsureError>;
}
