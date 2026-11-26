// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use velnor_runner_github::QueueSession;
use zeroize::Zeroizing;

use super::batch::Batch;
use super::cursor::Cursor;
use super::github::{Admin, QueueAccess};
use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::launch_blocking::ListenFault;

const ADMIN_RENEWAL: Duration = Duration::from_mins(20);
const ADMIN_RETRY: Duration = Duration::from_secs(30);

pub(super) struct Pending {
    pub(super) binding: RepositoryConnection,
    pub(super) pat: Zeroizing<String>,
    pub(super) policy: ApprovedJobAuthorityPolicy,
    pub(super) set: Option<(Admin, i64)>,
    pub(super) session: Option<QueueSession>,
    pub(super) cleared: bool,
}

pub(super) struct Live {
    pub(super) binding: RepositoryConnection,
    pub(super) pat: Zeroizing<String>,
    pub(super) policy: ApprovedJobAuthorityPolicy,
    pub(super) default_branch: Option<String>,
    pub(super) admin: Admin,
    pub(super) scale_set_id: i64,
    pub(super) session: QueueSession,
    pub(super) queue: QueueAccess,
    pub(super) cursor: Cursor,
    pub(super) polling: bool,
    pub(super) poll_retry_at: Option<Instant>,
    pub(super) batch: Option<Batch>,
    pub(super) refreshing: bool,
    pub(super) admin_due: Instant,
}

impl Pending {
    pub(super) fn into_live(self) -> Result<Live, ListenFault> {
        let (Some((admin, scale_set_id)), Some(session)) = (self.set, self.session) else {
            return Err(crate::error::HostError::RustRunner(
                crate::error::RustRunnerFailure::TaskFailed,
            )
            .into());
        };
        let queue = QueueAccess::of(&session, &admin)?;
        Ok(Live {
            binding: self.binding,
            pat: self.pat,
            policy: self.policy,
            default_branch: None,
            admin,
            scale_set_id,
            session,
            queue,
            cursor: Cursor::default(),
            polling: false,
            poll_retry_at: None,
            batch: None,
            refreshing: false,
            admin_due: Instant::now() + ADMIN_RENEWAL,
        })
    }
}

impl Live {
    pub(super) fn renewed(&mut self, now: Instant, admin: Option<Admin>) {
        self.refreshing = false;
        match admin {
            Some(admin) => {
                self.admin = admin;
                self.admin_due = now + ADMIN_RENEWAL;
            }
            None => self.admin_due = now + ADMIN_RETRY,
        }
    }

    pub(super) fn refreshed(
        &mut self,
        now: Instant,
        renewed: Option<(Admin, QueueSession)>,
    ) -> Result<(), ListenFault> {
        self.refreshing = false;
        let Some((admin, session)) = renewed else {
            self.poll_retry_at = Some(now + crate::launch::steady::RETRY);
            return Ok(());
        };
        self.queue = QueueAccess::of(&session, &admin)?;
        self.admin = admin;
        self.session = session;
        self.admin_due = now + ADMIN_RENEWAL;
        Ok(())
    }
}
