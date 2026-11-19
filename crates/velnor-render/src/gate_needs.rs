// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{JobId, When};

/// A job that must run after skipped ancestors needs an explicit status function
/// (GitHub otherwise adds `success()`, which a skipped ancestor turns false), so
/// it names the result of every job it needs instead.
pub(crate) fn succeeded(when: When, needs: &[JobId]) -> Vec<String> {
    match when {
        When::ScheduledAfterNeeds | When::DefaultBranchPushAfterNeeds => needs
            .iter()
            .map(|need| format!("needs.{need}.result == 'success'"))
            .collect(),
        _ => Vec::new(),
    }
}
