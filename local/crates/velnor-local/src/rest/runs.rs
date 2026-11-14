// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use crate::standin::{Job, Record, Run, conclusion};

fn second(time: Option<&String>) -> Option<String> {
    time.map(|time| format!("{}Z", time.get(..19).unwrap_or(time).trim_end_matches('Z')))
}

fn steps<'a>(job: &Record, records: &'a [Record]) -> Vec<&'a Record> {
    records
        .iter()
        .filter(|record| record.parent_id.as_deref() == Some(job.id.as_str()))
        .collect()
}

#[must_use]
pub fn job(job: &Job, records: &[Record]) -> Value {
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
    fn below(parent: &Record, records: &[Record], ids: &mut Vec<u64>) {
        for step in steps(parent, records) {
            ids.extend(step.log.as_ref().map(|log| log.id));
            below(step, records, ids);
        }
    }
    let mut ids = Vec::new();
    if let Some(root) = records.iter().find(|record| record.parent_id.is_none()) {
        below(root, records, &mut ids);
    }
    ids
}

#[must_use]
pub fn run(run: &Run, attempt: u32, jobs: &[Value]) -> Value {
    let times = |field: &'static str| jobs.iter().filter_map(move |job| job[field].as_str());
    let started = times("started_at").min().map(str::to_owned);
    json!({
        "id": run.id,
        "name": jobs.first().and_then(|job| job["workflow_name"].as_str()).or(run.display_name.as_deref()).unwrap_or(&run.file_name),
        "display_title": run.display_name,
        "status": run.status,
        "conclusion": conclusion(run.result.as_deref()),
        "run_attempt": attempt,
        "event": run.event_name,
        "head_sha": run.sha,
        "created_at": started.clone().unwrap_or_default(),
        "run_started_at": started,
        "updated_at": times("completed_at").max().unwrap_or_default(),
    })
}
