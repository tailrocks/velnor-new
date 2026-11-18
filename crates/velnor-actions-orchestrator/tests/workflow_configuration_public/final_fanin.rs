// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{REQUIRED_JOB_ID, Step};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::rendered_job_block;
use super::workflow_configuration_fixture;

const FETCH_REPORTS: &str = "Download every expected matrix artifact";
const ASSEMBLE_LEDGERS: &str = "Assemble group ledgers";
const WRITE_REQUEST: &str = "Write request";
const MERGE_REPORTS: &str = "Merge reports";

#[test]
fn generated_required_job_assembles_ledgers_after_fetch_and_before_merge()
-> Result<(), Box<dyn Error>> {
    let config = workflow_configuration_fixture::config_with_verification_branch(None);
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    let required = jobs.get(REQUIRED_JOB_ID).ok_or("Required job is missing")?;
    assert_step_order(&required.steps)?;

    let first = render_staged_tree(&prepared)?;
    let second = render_staged_tree(&prepared)?;
    let workflow = first
        .get(WORKFLOW_PATH)
        .ok_or("generated workflow is missing")?;
    assert_eq!(
        workflow,
        second.get(WORKFLOW_PATH).ok_or("workflow missing")?
    );
    let block = rendered_job_block(workflow, REQUIRED_JOB_ID)?;
    assert_rendered_order(block)?;
    Ok(())
}

fn assert_step_order(steps: &[Step]) -> Result<(), Box<dyn Error>> {
    let names = steps
        .iter()
        .map(|step| step.name.as_str())
        .collect::<Vec<_>>();
    assert_order(&names)
}

fn assert_rendered_order(block: &str) -> Result<(), Box<dyn Error>> {
    let names = [
        FETCH_REPORTS,
        ASSEMBLE_LEDGERS,
        WRITE_REQUEST,
        MERGE_REPORTS,
    ];
    let positions = names
        .iter()
        .map(|name| {
            let marker = format!("      - name: {name}\n");
            block
                .find(&marker)
                .ok_or_else(|| format!("generated Required job is missing {name}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        block
            .matches("      - name: Assemble group ledgers\n")
            .count(),
        1
    );
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    Ok(())
}

fn assert_order(names: &[&str]) -> Result<(), Box<dyn Error>> {
    let positions = [
        FETCH_REPORTS,
        ASSEMBLE_LEDGERS,
        WRITE_REQUEST,
        MERGE_REPORTS,
    ]
    .iter()
    .map(|wanted| {
        let matches = names.iter().filter(|name| **name == *wanted).count();
        if *wanted == ASSEMBLE_LEDGERS && matches != 1 {
            return Err(format!("expected one assemble step, found {matches}"));
        }
        names
            .iter()
            .position(|name| name == wanted)
            .ok_or_else(|| format!("Required job is missing {wanted}"))
    })
    .collect::<Result<Vec<_>, _>>()?;
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    Ok(())
}
