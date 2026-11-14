// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: u64,
    pub file_name: String,
    pub display_name: Option<String>,
    pub event_name: Option<String>,
    pub sha: Option<String>,
    pub result: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub request_id: u64,
    pub job_id: String,
    pub time_line_id: Option<String>,
    pub name: String,
    pub workflowname: String,
    pub result: Option<String>,
    pub attempt: u32,
    pub runid: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub start_time: Option<String>,
    pub finish_time: Option<String>,
    pub result: Option<String>,
    pub log: Option<RecordLog>,
}

#[derive(Debug, Deserialize)]
pub struct RecordLog {
    pub id: u64,
}

#[must_use]
pub fn conclusion(result: Option<&str>) -> Option<&'static str> {
    Some(match result? {
        "succeeded" | "succeededWithIssues" => "success",
        "failed" => "failure",
        "skipped" => "skipped",
        "canceled" | "cancelled" | "abandoned" => "cancelled",
        _ => "neutral",
    })
}

#[must_use]
pub fn seconds(time: &str) -> Option<u64> {
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

#[derive(Debug)]
pub struct ClientCall<'a> {
    pub event: &'a str,
    pub payload: &'a str,
    pub git_ref: &'a str,
    pub sha: &'a str,
    pub repository: &'a str,
    pub workflow: Option<&'a str>,
}

impl<'a> ClientCall<'a> {
    #[must_use]
    pub fn args(&self) -> Vec<&'a str> {
        let mut args = vec![
            "/opt/runner.server/Runner.Client.dll",
            "--server",
            "http://runner-server",
            "--event",
            self.event,
            "--payload",
            self.payload,
            "--ref",
            self.git_ref,
            "--sha",
            self.sha,
            "--repository",
            self.repository,
            "--no-default-payload",
        ];
        if let Some(workflow) = self.workflow {
            args.extend(["-W", workflow]);
        }
        args
    }
}
