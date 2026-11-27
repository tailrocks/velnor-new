// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::Result;

use super::exec_support::Scratch;
use super::lifecycle_cancel::{assert_stopped_at_the_deadline, hanging_cleanup};
use super::lifecycle_support::{RUN_BUDGET, Settings, run_in};
use super::support::mock_github::{Behavior, Cancel, MockGithub};

const FLOOR: Duration = Duration::from_secs(16);
const LENIENT_WINDOW: Duration = Duration::from_secs(30);

async fn out_of_range_timeout_falls_back_to_the_floor(text: &'static str) -> Result<()> {
    let behavior = Behavior {
        cancel: Cancel::After(Duration::from_millis(300)),
        cancel_timeout_text: Some(text),
        ..Behavior::default()
    };
    let scratch = Scratch::new()?;
    let mock = MockGithub::start(hanging_cleanup(&scratch)?, behavior).await?;
    let started = Instant::now();

    let outcome = run_in(
        &mock,
        &scratch,
        Settings::new()
            .killing_after_cancel_floor(FLOOR)
            .renewing_every(Duration::from_secs(60))
            .within(RUN_BUDGET),
    )
    .await??;

    assert!(
        started.elapsed() < LENIENT_WINDOW,
        "{:?}",
        started.elapsed()
    );
    assert_stopped_at_the_deadline(&mock, &scratch, outcome)
}

#[tokio::test]
async fn lifecycle_cancel_with_sixty_seconds_in_the_timeout_uses_the_floor() -> Result<()> {
    out_of_range_timeout_falls_back_to_the_floor("00:00:60").await
}

#[tokio::test]
async fn lifecycle_cancel_with_sixty_minutes_in_the_timeout_uses_the_floor() -> Result<()> {
    out_of_range_timeout_falls_back_to_the_floor("00:60:00").await
}

#[tokio::test]
async fn lifecycle_cancel_with_twenty_four_hours_in_the_timeout_uses_the_floor() -> Result<()> {
    out_of_range_timeout_falls_back_to_the_floor("24:00:00").await
}

#[tokio::test]
async fn lifecycle_cancel_with_more_days_than_a_time_span_holds_uses_the_floor() -> Result<()> {
    out_of_range_timeout_falls_back_to_the_floor("10675200.00:00:00").await
}
