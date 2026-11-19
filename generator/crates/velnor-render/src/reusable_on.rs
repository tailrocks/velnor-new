// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_workflows::{Input, InputKind, Output, Triggers};

use crate::yaml::{Yaml, entry, text};

/// The `on:` mapping, in one fixed order whatever order the config lists the
/// events in.
pub(crate) fn on(triggers: &Triggers) -> Yaml {
    let mut items = Vec::new();
    if let Some(call) = &triggers.workflow_call {
        let mut keys = Vec::new();
        if !call.inputs.is_empty() {
            keys.push(entry("inputs", inputs(&call.inputs)));
        }
        if !call.outputs.is_empty() {
            keys.push(entry("outputs", outputs(&call.outputs)));
        }
        items.push(entry("workflow_call", Yaml::Map(keys)));
    }
    if let Some(dispatch) = &triggers.workflow_dispatch {
        let keys = if dispatch.is_empty() {
            Vec::new()
        } else {
            vec![entry("inputs", inputs(dispatch))]
        };
        items.push(entry("workflow_dispatch", Yaml::Map(keys)));
    }
    if !triggers.schedule.is_empty() {
        items.push(entry(
            "schedule",
            Yaml::Seq(
                triggers
                    .schedule
                    .iter()
                    .map(|cron| Yaml::Map(vec![entry("cron", text(cron))]))
                    .collect(),
            ),
        ));
    }
    if let Some(push) = &triggers.push {
        items.push(entry(
            "push",
            filters(&[("branches", &push.branches), ("tags", &push.tags)]),
        ));
    }
    if let Some(request) = &triggers.pull_request {
        items.push(entry(
            "pull_request",
            filters(&[("types", &request.types), ("branches", &request.branches)]),
        ));
    }
    if triggers.merge_group {
        items.push(entry("merge_group", Yaml::Map(Vec::new())));
    }
    if let Some(run) = &triggers.workflow_run {
        items.push(entry(
            "workflow_run",
            filters(&[
                ("workflows", &run.workflows),
                ("types", &run.types),
                ("branches", &run.branches),
            ]),
        ));
    }
    Yaml::Map(items)
}

fn filters(lists: &[(&str, &Vec<String>)]) -> Yaml {
    Yaml::Map(
        lists
            .iter()
            .filter(|(_, values)| !values.is_empty())
            .map(|(key, values)| {
                entry(
                    key,
                    Yaml::Seq(values.iter().map(|value| text(value)).collect()),
                )
            })
            .collect(),
    )
}

fn inputs(inputs: &[Input]) -> Yaml {
    Yaml::Map(
        inputs
            .iter()
            .map(|input| {
                let mut items = Vec::new();
                if let Some(description) = &input.description {
                    items.push(entry("description", text(description)));
                }
                items.push(entry("required", Yaml::Bool(input.required)));
                items.push(entry("type", text(input.kind.name())));
                if let Some(default) = &input.default {
                    items.push(entry("default", default_value(&input.kind, default)));
                }
                if let InputKind::Choice(options) = &input.kind {
                    items.push(entry(
                        "options",
                        Yaml::Seq(options.iter().map(|option| text(option)).collect()),
                    ));
                }
                entry(&input.name, Yaml::Map(items))
            })
            .collect(),
    )
}

fn default_value(kind: &InputKind, value: &str) -> Yaml {
    match (kind, value, value.parse::<u64>()) {
        (InputKind::Boolean, flag, _) => Yaml::Bool(flag == "true"),
        (InputKind::Number, _, Ok(number)) => Yaml::Int(number),
        _ => text(value),
    }
}

fn outputs(outputs: &[Output]) -> Yaml {
    Yaml::Map(
        outputs
            .iter()
            .map(|output| {
                let mut items = Vec::new();
                if let Some(description) = &output.description {
                    items.push(entry("description", text(description)));
                }
                items.push(entry(
                    "value",
                    Yaml::Expr(format!(
                        "${{{{ jobs.{}.outputs.{} }}}}",
                        output.from_job, output.from_output
                    )),
                ));
                entry(&output.name, Yaml::Map(items))
            })
            .collect(),
    )
}
