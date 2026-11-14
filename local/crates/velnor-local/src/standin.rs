// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: u64,
    pub display_name: String,
    pub event_name: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub time_line_id: Option<String>,
    pub workflowname: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub parent_id: Option<String>,
    pub start_time: Option<String>,
}

#[must_use]
pub fn conclusion(result: Option<&str>) -> &'static str {
    match result {
        Some("succeeded" | "succeededWithIssues") => "success",
        Some("failed") => "failure",
        Some("skipped") => "skipped",
        Some("canceled" | "cancelled" | "abandoned") => "cancelled",
        Some(_) => "neutral",
        None => "unfinished",
    }
}

#[must_use]
pub fn job_started(records: &[Record]) -> Option<u64> {
    let root = records.iter().find(|record| record.parent_id.is_none())?;
    seconds(root.start_time.as_deref()?)
}

fn seconds(time: &str) -> Option<u64> {
    let number = |range: std::ops::Range<usize>| time.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
        + day_of_year
        - 719_468;
    let clock = number(11..13)? * 3600 + number(14..16)? * 60 + number(17..19)?;
    u64::try_from(days * 86_400 + clock).ok()
}
