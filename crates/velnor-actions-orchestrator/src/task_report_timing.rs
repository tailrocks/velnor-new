// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::internal::internal;

pub(super) fn parse_exit_code(raw: &str) -> Result<i32, OrchestratorError> {
    raw.parse::<i32>()
        .ok()
        .filter(|code| (0..=255).contains(code))
        .ok_or_else(|| internal("bad_exit_code"))
}

pub(super) fn parse_start_ms(raw: &str) -> Option<u64> {
    raw.parse::<u64>().ok()
}

pub(super) fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
}

pub(super) fn elapsed_ms(start_ms: Option<u64>) -> Option<u64> {
    let (Some(start), Some(now)) = (start_ms, now_ms()) else {
        return None;
    };
    now.checked_sub(start).map(|elapsed| elapsed.max(1))
}
