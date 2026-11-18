// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_model::{DeclaredBody, DeclaredJob, DeclaredStep, DeclaredWorkflow};

use crate::error::Reason;

type Table = BTreeMap<String, String>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawWorkflow {
    name: String,
    file: String,
    on: OneOrMany,
    #[serde(default)]
    permissions: Table,
    #[serde(rename = "if")]
    condition: Option<String>,
    job: Vec<RawJob>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJob {
    id: String,
    #[serde(default)]
    needs: Vec<String>,
    environment: Option<String>,
    #[serde(default)]
    permissions: Table,
    timeout_minutes: Option<u16>,
    #[serde(rename = "if")]
    condition: Option<String>,
    #[serde(default)]
    outputs: Table,
    step: Vec<RawStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    id: Option<String>,
    name: Option<String>,
    #[serde(rename = "if")]
    condition: Option<String>,
    uses: Option<String>,
    helper: Option<String>,
    run: Option<String>,
    #[serde(default)]
    with: Table,
    #[serde(default)]
    env: Table,
}

pub(crate) fn parse(raw: Vec<RawWorkflow>) -> Result<Vec<DeclaredWorkflow>, Reason> {
    raw.into_iter().map(workflow).collect()
}

fn pairs(table: Table) -> Vec<(String, String)> {
    table.into_iter().collect()
}

fn workflow(raw: RawWorkflow) -> Result<DeclaredWorkflow, Reason> {
    Ok(DeclaredWorkflow {
        name: raw.name,
        file: raw.file,
        on: match raw.on {
            OneOrMany::One(event) => vec![event],
            OneOrMany::Many(events) => events,
        },
        permissions: pairs(raw.permissions),
        condition: raw.condition,
        jobs: raw.job.into_iter().map(job).collect::<Result<_, _>>()?,
    })
}

fn job(raw: RawJob) -> Result<DeclaredJob, Reason> {
    Ok(DeclaredJob {
        id: raw.id,
        needs: raw.needs,
        environment: raw.environment,
        permissions: pairs(raw.permissions),
        timeout_minutes: raw.timeout_minutes,
        condition: raw.condition,
        outputs: pairs(raw.outputs),
        steps: raw.step.into_iter().map(step).collect::<Result<_, _>>()?,
    })
}

fn step(raw: RawStep) -> Result<DeclaredStep, Reason> {
    let label = || {
        raw.id
            .clone()
            .or_else(|| raw.name.clone())
            .unwrap_or_default()
    };
    if raw.run.is_some() {
        return Err(Reason::InvalidValue {
            key: "workflow.job.step.run (free run strings are refused; use uses or helper)",
            value: label(),
        });
    }
    let body = match (&raw.uses, &raw.helper) {
        (Some(action), None) => DeclaredBody::Action(action.clone()),
        (None, Some(command)) => DeclaredBody::Helper(command.clone()),
        _ => {
            return Err(Reason::InvalidValue {
                key: "workflow.job.step",
                value: label(),
            });
        }
    };
    Ok(DeclaredStep {
        id: raw.id,
        name: raw.name,
        condition: raw.condition,
        body,
        with: pairs(raw.with),
        env: pairs(raw.env),
    })
}
