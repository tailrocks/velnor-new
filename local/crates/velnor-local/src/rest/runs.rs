// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandInRun {
    pub id: u64,
    pub display_name: String,
    pub event_name: Option<String>,
    pub sha: Option<String>,
    pub result: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandInJob {
    pub request_id: u64,
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
    pub order: Option<i64>,
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

fn second(time: Option<&String>) -> Option<String> {
    time.map(|time| format!("{}Z", &time[..time.len().min(19)]))
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

fn steps<'a>(job: &Record, records: &'a [Record]) -> Vec<&'a Record> {
    let mut steps: Vec<&Record> = records
        .iter()
        .filter(|record| record.parent_id.as_deref() == Some(job.id.as_str()))
        .collect();
    steps.sort_by_key(|record| record.order);
    steps
}

#[must_use]
pub fn job(job: &StandInJob, records: &[Record]) -> Value {
    let root = records.iter().find(|record| record.parent_id.is_none());
    let started = root.and_then(|root| second(root.start_time.as_ref()));
    let steps: Vec<Value> = root
        .map(|root| steps(root, records))
        .unwrap_or_default()
        .into_iter()
        .zip(1..)
        .map(|(step, number): (&Record, u32)| {
            json!({
                "name": step.name,
                "number": number,
                "status": "completed",
                "conclusion": conclusion(step.result.as_deref()),
                "started_at": second(step.start_time.as_ref()),
                "completed_at": second(step.finish_time.as_ref()),
            })
        })
        .collect();
    json!({
        "id": job.request_id,
        "run_id": job.runid,
        "run_attempt": job.attempt,
        "name": job.name,
        "workflow_name": job.workflowname,
        "status": if job.result.is_some() { "completed" } else { "in_progress" },
        "conclusion": conclusion(job.result.as_deref()),
        "created_at": started.clone().unwrap_or_default(),
        "started_at": started,
        "completed_at": root.and_then(|root| second(root.finish_time.as_ref())),
        "steps": steps,
    })
}

#[must_use]
pub fn log_ids(records: &[Record]) -> Vec<u64> {
    records
        .iter()
        .find(|record| record.parent_id.is_none())
        .map(|root| steps(root, records))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|step| step.log.as_ref().map(|log| log.id))
        .collect()
}

#[must_use]
pub fn run(run: &StandInRun, jobs: &[Value]) -> Value {
    let times = |field: &'static str| jobs.iter().filter_map(move |job| job[field].as_str());
    let started = times("started_at").min().map(str::to_owned);
    json!({
        "id": run.id,
        "name": jobs.first().and_then(|job| job["workflow_name"].as_str()).unwrap_or(&run.display_name),
        "display_title": run.display_name,
        "status": run.status,
        "conclusion": conclusion(run.result.as_deref()),
        "run_attempt": 1,
        "event": run.event_name,
        "head_sha": run.sha,
        "created_at": started.clone().unwrap_or_default(),
        "run_started_at": started,
        "updated_at": times("completed_at").max().unwrap_or_default(),
    })
}
