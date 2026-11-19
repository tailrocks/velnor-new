// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::reconcile::IntentRow;

pub(crate) const MAX_COMPLETION_BODY_BYTES: usize = velnor_runner_github::MAX_POLL_BODY_BYTES;
pub(crate) const MAX_COMPLETION_INBOX_SCAN: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CleanupClaim {
    pub(crate) generation: i64,

    pub(crate) attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionIdentity {
    pub(crate) scale_set_id: i64,

    pub(crate) runner_request_id: i64,

    pub(crate) runner_id: i64,

    pub(crate) runner_name: String,

    pub(crate) runner_absent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletedLaunch {
    pub(crate) intent: IntentRow,

    pub(crate) identity: CompletionIdentity,
}
