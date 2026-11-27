// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use super::native_artifact_zones::{Clock, PACIFIC, SYDNEY, Setup, assert_near, expiry_for};

const REJECTED_BY_ICU: [&str; 14] = [
    "posix/PST8PDT",
    "right/PST8PDT",
    ":posix/PST8PDT",
    ":right/PST8PDT",
    "posix/EST5EDT",
    "posix/MST7MDT",
    "posix/CST6CDT",
    "::PST8PDT",
    " PST8PDT",
    ":PST8PDT",
    ":EST5EDT",
    ":MST7MDT",
    ":CST6CDT",
    "PST8PDT ",
];

const EXEMPT_FROM_THE_DIGIT_RULE: [(&str, &str); 4] = [
    ("PST8PDT", PACIFIC),
    ("EST5EDT", "America/New_York"),
    ("MST7MDT", "America/Denver"),
    ("CST6CDT", "America/Chicago"),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_value_icu_rejects_leaves_the_host_zone_in_force_whatever_prefix_hides_a_name()
-> Result<()> {
    let clock = Clock::pick(&[PACIFIC, SYDNEY])?;
    let expected = clock.calendar(SYDNEY)?;
    for row in REJECTED_BY_ICU {
        let sent = expiry_for(&clock, &[], Some(row), Setup::HOST_SYDNEY).await?;
        assert_near(sent, expected, row, &clock);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_value_icu_rejects_adds_whole_days_when_the_host_has_no_zone() -> Result<()> {
    let clock = Clock::pick(&[PACIFIC, SYDNEY])?;
    for row in &REJECTED_BY_ICU[..9] {
        let sent = expiry_for(&clock, &[], Some(row), Setup::NO_HOST_ZONE).await?;
        assert_near(sent, clock.flat, row, &clock);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_four_exempt_names_resolve_whole_though_they_hold_a_digit() -> Result<()> {
    for (row, zone) in EXEMPT_FROM_THE_DIGIT_RULE {
        let clock = Clock::pick(&[zone, SYDNEY])?;
        let sent = expiry_for(&clock, &[], Some(row), Setup::HOST_SYDNEY).await?;
        assert_near(sent, clock.calendar(zone)?, row, &clock);
    }
    Ok(())
}
