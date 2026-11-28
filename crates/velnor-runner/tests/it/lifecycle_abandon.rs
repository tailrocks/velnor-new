// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::Result;

use super::exec_support::Scratch;
use super::lifecycle_cancel::{assert_stopped_at_the_deadline, hanging_cleanup, step_started_file};
use super::lifecycle_support::{RUN_BUDGET, Settings, run_until};
use super::support::mock_github::{Behavior, MockGithub, RESULTS_STEPS_UPDATE, StepStatus};

const REPORT_BUDGET: Duration = Duration::from_secs(2);
const LATE_WINDOW: Duration = Duration::from_secs(3);

#[tokio::test]
async fn lifecycle_sends_no_step_update_after_the_completion_of_an_abandoned_job() -> Result<()> {
    let scratch = Scratch::new()?;
    let behavior = Behavior {
        status: StepStatus::Stalls,
        ..Behavior::default()
    };
    let mock = MockGithub::start(hanging_cleanup(&scratch)?, behavior).await?;
    let settings = Settings::new()
        .killing_after_cancel_floor(Duration::from_secs(16))
        .reporting_within(REPORT_BUDGET)
        .renewing_every(Duration::from_secs(60))
        .within(RUN_BUDGET);
    let started = step_started_file(&scratch);

    let outcome = run_until(&mock, &scratch, settings, "the step to start", || {
        Ok(started.exists())
    })
    .await??;
    tokio::time::sleep(LATE_WINDOW).await;

    assert_stopped_at_the_deadline(&mock, &scratch, outcome)?;
    let completed = mock
        .server
        .only_request("POST", "/run/completejob")?
        .arrived();
    let updates = mock.requests_to("POST", RESULTS_STEPS_UPDATE)?;
    let late = updates
        .iter()
        .filter(|update| update.arrived() > completed)
        .count();
    assert!(!updates.is_empty(), "the job reported no step at all");
    assert_eq!(late, 0, "step updates arrived after the completion");
    Ok(())
}
