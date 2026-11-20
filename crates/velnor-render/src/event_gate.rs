// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::When;

use crate::gate::ON_DEFAULT_BRANCH;

const SCHEDULE: &str = "schedule";
const PUSH: &str = "push";

/// The events a job that runs on a schedule or a push runs on.
pub(crate) fn event_gate(when: When) -> String {
    match when {
        When::Scheduled => format!("github.event_name == '{SCHEDULE}'"),
        When::ScheduledOrDefaultBranchPushAfterNeeds => format!(
            "(github.event_name == '{SCHEDULE}' || {})",
            event_gate(When::DefaultBranchPush)
        ),
        When::ScheduledOrPush => {
            format!("(github.event_name == '{SCHEDULE}' || github.event_name == '{PUSH}')")
        }
        _ => format!("(github.event_name == '{PUSH}' && {ON_DEFAULT_BRANCH})"),
    }
}
