// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

pub(super) const RECOVERY_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const RECOVERY_POLL: Duration = Duration::from_millis(100);
pub(super) const RECOVERY_REQUEST: Duration = Duration::from_secs(1);

pub(super) enum Recovery {
    Resolved,
    Unresolved { owned_observed: bool },
}
