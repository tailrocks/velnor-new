// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{error::HostError, journal::JitConflictRecoveryClaim};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const OPERATION_CAP_SECONDS: u64 = 60;
const CLAIM_LEASE_SAFETY_MARGIN_SECONDS: i64 = 15;

pub(super) fn operation_deadline(claim: &JitConflictRecoveryClaim) -> Result<Instant, HostError> {
    let monotonic_now = Instant::now();
    let wall_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Identity)?;
    let now_seconds = i64::try_from(wall_now.as_secs()).map_err(|_| HostError::Identity)?;
    operation_deadline_at(claim.lease_until(), now_seconds, monotonic_now)
}

fn operation_deadline_at(
    lease_until: i64,
    now_seconds: i64,
    monotonic_now: Instant,
) -> Result<Instant, HostError> {
    let remaining = lease_until
        .checked_sub(now_seconds)
        .and_then(|seconds| seconds.checked_sub(CLAIM_LEASE_SAFETY_MARGIN_SECONDS))
        .filter(|seconds| *seconds > 0)
        .ok_or(HostError::Identity)?;
    let budget = Duration::from_secs(u64::try_from(remaining).map_err(|_| HostError::Identity)?)
        .min(Duration::from_secs(OPERATION_CAP_SECONDS));
    monotonic_now.checked_add(budget).ok_or(HostError::Identity)
}
