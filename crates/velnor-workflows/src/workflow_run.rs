// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};
use crate::plan::Workflow;

const MAX_CHAIN: usize = 3;

pub(crate) fn check_chains(workflows: &[Workflow]) -> Result<(), WorkflowsError> {
    for flow in workflows {
        let links = links(flow, workflows, &mut Vec::new())?;
        if links > MAX_CHAIN {
            return refuse(format!(
                "the workflow {} starts after a chain of {links} workflow_run triggers; GitHub runs at most {MAX_CHAIN} chained levels, so the later workflows never start",
                flow.file
            ));
        }
    }
    Ok(())
}

fn links<'a>(
    flow: &'a Workflow,
    workflows: &'a [Workflow],
    path: &mut Vec<&'a str>,
) -> Result<usize, WorkflowsError> {
    let Some(run) = &flow.triggers.workflow_run else {
        return Ok(0);
    };
    if path.contains(&flow.file.as_str()) {
        return refuse(format!(
            "the workflow {} follows itself through workflow_run triggers",
            flow.file
        ));
    }
    path.push(&flow.file);
    let mut longest = 0;
    for followed in workflows
        .iter()
        .filter(|other| run.workflows.contains(&other.name))
    {
        longest = longest.max(links(followed, workflows, path)?);
    }
    path.pop();
    Ok(1 + longest)
}
