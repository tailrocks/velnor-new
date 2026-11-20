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
