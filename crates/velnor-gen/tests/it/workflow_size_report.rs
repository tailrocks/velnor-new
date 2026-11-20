// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, render_workflow};

const STEPS: usize = 2_600;

fn job(id: &str, steps: usize) -> RenderedJob {
    RenderedJob {
        id: id.to_owned(),
        name: Some(id.to_owned()),
        runs_on: "ubuntu-latest".to_owned(),
        timeout_minutes: 10,
        steps: (0..steps)
            .map(|index| RenderedStep {
                name: format!("step {index} {}", "n".repeat(150)),
                run: Some("echo ok".to_owned()),
                ..RenderedStep::default()
            })
            .collect(),
        ..RenderedJob::default()
    }
}

#[test]
fn the_largest_jobs_reported_for_an_oversized_workflow_add_up_to_its_bytes() -> TestResult {
    let plan = crate::rules::plan(Vec::new())?;
    let rendered = RenderedWorkflow {
        jobs: vec![job("heavy", STEPS), job("light", 1)],
    };
    let errors = render_workflow(&plan, &rendered)
        .err()
        .ok_or("the workflow should be over the size limit")?;
    let violation = errors
        .iter()
        .find(|violation| matches!(violation.rule, Rule::WorkflowTooLarge(_)))
        .ok_or("no size violation")?;
    let Rule::WorkflowTooLarge(file_bytes) = violation.rule else {
        return Err("no size violation".into());
    };
    let (_, list) = violation
        .job
        .split_once("the largest are ")
        .ok_or("no largest jobs list")?;
    let reported = list
        .split(", ")
        .map(|entry| {
            entry
                .strip_suffix(" bytes")
                .and_then(|rest| rest.rsplit_once(' '))
                .ok_or_else(|| format!("unparsed entry {entry}"))?
                .1
                .parse::<usize>()
                .map_err(|error| error.to_string())
        })
        .sum::<Result<usize, String>>()?;
    assert!(
        file_bytes.abs_diff(reported) * 100 <= file_bytes,
        "jobs report {reported} bytes, the file is {file_bytes} bytes: {}",
        violation.job
    );
    Ok(())
}

fn largest_jobs(job_text: &str) -> Result<Vec<(String, usize)>, String> {
    let (_, list) = job_text
        .split_once("the largest are ")
        .ok_or("no largest jobs list")?;
    list.split(", ")
        .map(|entry| {
            let (id, bytes) = entry
                .strip_suffix(" bytes")
                .and_then(|rest| rest.rsplit_once(' '))
                .ok_or_else(|| format!("unparsed entry {entry}"))?;
            let bytes = bytes.parse::<usize>().map_err(|error| error.to_string())?;
            Ok((id.to_owned(), bytes))
        })
        .collect()
}

fn size_violation(jobs: Vec<RenderedJob>) -> Result<String, Box<dyn std::error::Error>> {
    let plan = crate::rules::plan(Vec::new())?;
    let errors = render_workflow(&plan, &RenderedWorkflow { jobs })
        .err()
        .ok_or("the workflow should be over the size limit")?;
    let violation = errors
        .into_iter()
        .find(|violation| matches!(violation.rule, Rule::WorkflowTooLarge(_)))
        .ok_or("no size violation")?;
    Ok(violation.job)
}

#[test]
fn a_comment_above_the_first_job_is_charged_to_that_job() -> TestResult {
    let note = "x".repeat(40_000);
    let first = RenderedJob {
        comment: Some(note.clone()),
        ..job("first", 1)
    };
    let text = size_violation(vec![first, job("heavy", STEPS)])?;
    let sizes = largest_jobs(&text)?;
    let (_, first_bytes) = sizes
        .iter()
        .find(|(id, _)| id == "first")
        .ok_or("first job is not in the largest list")?;
    assert!(*first_bytes >= note.len(), "first is {first_bytes} bytes: {text}");
    Ok(())
}

#[test]
fn a_comment_above_a_later_job_is_not_charged_to_the_job_before_it() -> TestResult {
    let note = "x".repeat(40_000);
    let last = RenderedJob {
        comment: Some(note.clone()),
        ..job("last", 1)
    };
    let text = size_violation(vec![job("heavy", STEPS), last])?;
    let sizes = largest_jobs(&text)?;
    let (_, last_bytes) = sizes
        .iter()
        .find(|(id, _)| id == "last")
        .ok_or("last job is not in the largest list")?;
    assert!(*last_bytes >= note.len(), "last is {last_bytes} bytes: {text}");
    Ok(())
}
