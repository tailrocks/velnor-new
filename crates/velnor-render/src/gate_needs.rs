// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{JobId, When};

pub(crate) fn succeeded(when: When, needs: &[JobId]) -> Vec<String> {
    match when {
        When::ScheduledOrDefaultBranchPushAfterNeeds | When::DefaultBranchPushAfterNeeds => needs
            .iter()
            .map(|need| format!("needs.{need}.result == 'success'"))
            .collect(),
        When::FinalizerAfterProducer => vec![format!(
            "({})",
            needs
                .iter()
                .map(|need| format!("needs.{need}.result != 'skipped'"))
                .collect::<Vec<_>>()
                .join(" || ")
        )],
        _ => Vec::new(),
    }
}
