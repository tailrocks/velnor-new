// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::github_hosted;

use crate::check::runner_label_of;
use crate::error::{WorkflowsError, refuse};
use crate::expression::is_identifier;
use crate::plan::{InputKind, Triggers};
use crate::raw::{RawCase, RawJob};

const MODE_INPUT: &str = "mode";

/// Expands every case into a hosted job and a scale-set job that share the
/// steps and are selected by the same `inputs.mode` condition.
pub(crate) fn cases(
    file: &str,
    hosted: Option<&str>,
    scale_set: Option<&str>,
    raw: Vec<RawCase>,
    triggers: &Triggers,
) -> Result<Vec<RawJob>, WorkflowsError> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let options = mode_options(file, triggers)?;
    let hosted = pair_runner(file, "hosted_runner", hosted, true)?;
    let scale_set = pair_runner(file, "scale_set_runner", scale_set, false)?;
    let mut seen: Vec<String> = Vec::new();
    let mut jobs = Vec::new();
    for case in raw {
        if seen.contains(&case.id) {
            return refuse(format!("the case {} of {file} is declared twice", case.id));
        }
        seen.push(case.id.clone());
        jobs.extend(pair(file, options, &case, &hosted, &scale_set)?);
    }
    Ok(jobs)
}

fn mode_options<'a>(file: &str, triggers: &'a Triggers) -> Result<&'a [String], WorkflowsError> {
    let input = triggers
        .workflow_dispatch
        .iter()
        .flatten()
        .find(|input| input.name == MODE_INPUT);
    match input.map(|input| &input.kind) {
        Some(InputKind::Choice(options)) => Ok(options),
        _ => refuse(format!(
            "the cases of {file} need a workflow_dispatch input {MODE_INPUT} of type choice"
        )),
    }
}

fn pair_runner(
    file: &str,
    key: &str,
    label: Option<&str>,
    hosted: bool,
) -> Result<String, WorkflowsError> {
    let Some(label) = label else {
        return refuse(format!("the cases of {file} need {key}"));
    };
    let label = runner_label_of(file, label)?;
    if github_hosted(&label) != hosted {
        let kind = if hosted {
            "GitHub-hosted"
        } else {
            "not GitHub-hosted"
        };
        return refuse(format!("the {key} {label} of {file} must be {kind}"));
    }
    Ok(label)
}

fn pair(
    file: &str,
    options: &[String],
    case: &RawCase,
    hosted: &str,
    scale_set: &str,
) -> Result<[RawJob; 2], WorkflowsError> {
    let id = &case.id;
    if !is_identifier(id, true) {
        return refuse(format!("the case id {id:?} of {file} is not an identifier"));
    }
    if case.step.is_empty() {
        return refuse(format!("the case {id} of {file} needs at least one step"));
    }
    let modes = if case.modes.is_empty() {
        vec![id.clone()]
    } else {
        case.modes.clone()
    };
    if let Some(unknown) = modes.iter().find(|mode| !options.contains(mode)) {
        return refuse(format!(
            "the case {id} of {file} runs in mode {unknown}, which the {MODE_INPUT} input does not offer"
        ));
    }
    let selected = modes
        .iter()
        .map(|mode| format!("{MODE_INPUT_REF} == '{mode}'"))
        .collect::<Vec<_>>()
        .join(" || ");
    let condition = match &case.condition {
        Some(extra) => format!("({selected}) && ({extra})"),
        None => selected,
    };
    let title = case.title.clone().unwrap_or_else(|| id.clone());
    let job = |suffix: &str, label: &str, runner: &str| RawJob {
        id: format!("{id}-{suffix}"),
        name: Some(format!("{title} / {label}")),
        needs: Vec::new(),
        condition: Some(condition.clone()),
        helper: false,
        runner: Some(runner.to_owned()),
        environment: None,
        timeout_minutes: case.timeout_minutes,
        concurrency: None,
        permissions: case.permissions.clone(),
        env: case.env.clone(),
        cache_mode: case.cache_mode.clone(),
        outputs: BTreeMap::new(),
        matrix: None,
        uses: None,
        with: BTreeMap::new(),
        container: case.container.clone(),
        service: case.service.clone(),
        step: case.step.clone(),
    };
    Ok([
        job("hosted", "GitHub hosted", hosted),
        job("scale-set", "Velnor Scale Set", scale_set),
    ])
}

const MODE_INPUT_REF: &str = "inputs.mode";
