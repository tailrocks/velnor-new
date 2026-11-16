// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::yaml::{Yaml, entry, text};
use velnor_model::TESTS_RUN_OUTPUT;

pub(crate) fn yaml_job(job: &RenderedJob) -> Yaml {
    let mut items = Vec::new();
    if let Some(name) = &job.name {
        items.push(entry("name", text(name)));
    }
    if !job.needs.is_empty() {
        items.push(entry(
            "needs",
            Yaml::Seq(job.needs.iter().map(|need| text(need)).collect()),
        ));
    }
    if let Some(condition) = &job.condition {
        items.push(entry("if", Yaml::Expr(condition.clone())));
    }
    if let Some(environment) = &job.environment {
        items.push(entry("environment", text(environment)));
    }
    if job.no_permissions {
        items.push(entry("permissions", Yaml::Map(Vec::new())));
    } else if !job.permissions.is_empty() {
        items.push(entry("permissions", pairs(&job.permissions)));
    }
    if let Some(concurrency) = &job.concurrency {
        items.push(entry(
            "concurrency",
            Yaml::Map(vec![
                entry("group", Yaml::Expr(concurrency.group.clone())),
                entry(
                    "cancel-in-progress",
                    Yaml::Bool(concurrency.cancel_in_progress),
                ),
            ]),
        ));
    }
    if !job.outputs.is_empty() {
        items.push(entry(
            "outputs",
            Yaml::Map(
                job.outputs
                    .iter()
                    .map(|(name, value)| {
                        let value = value.clone();
                        entry(
                            name,
                            if name == TESTS_RUN_OUTPUT {
                                Yaml::Unread(value)
                            } else {
                                Yaml::Expr(value)
                            },
                        )
                    })
                    .collect(),
            ),
        ));
    }
    items.extend([
        entry("runs-on", text(&job.runs_on)),
        entry("timeout-minutes", Yaml::Int(u64::from(job.timeout_minutes))),
    ]);
    items.extend([entry(
        "steps",
        Yaml::Seq(job.steps.iter().map(yaml_step).collect()),
    )]);
    match &job.comment {
        Some(comment) => Yaml::Noted(comment.clone(), Box::new(Yaml::Map(items))),
        None => Yaml::Map(items),
    }
}

fn yaml_step(step: &RenderedStep) -> Yaml {
    let mut items = Vec::new();
    if let Some(id) = &step.id {
        items.push(entry("id", text(id)));
    }
    items.push(entry("name", text(&step.name)));
    if let Some(condition) = &step.condition {
        items.push(entry("if", Yaml::Expr(condition.clone())));
    }
    if let Some(uses) = &step.uses {
        let value = match &step.uses_comment {
            Some(comment) => Yaml::Trailed(comment.clone(), Box::new(text(uses))),
            None => text(uses),
        };
        items.push(entry("uses", value));
    }
    if !step.with.is_empty() {
        items.push(entry("with", values(&step.with)));
    }
    if let Some(directory) = &step.working_directory {
        items.push(entry("working-directory", text(directory)));
    }
    if !step.env.is_empty() {
        items.push(entry("env", values(&step.env)));
    }
    if let Some(shell) = &step.shell {
        items.push(entry("shell", text(shell)));
    }
    if let Some(run) = &step.run {
        let value = if run.contains('\n') {
            Yaml::Block(run.clone())
        } else {
            text(run)
        };
        items.push(entry("run", value));
    }
    Yaml::Map(items)
}

fn pairs(items: &[(String, String)]) -> Yaml {
    Yaml::Map(
        items
            .iter()
            .map(|(key, value)| entry(key, text(value)))
            .collect(),
    )
}

fn values(items: &[(String, Value)]) -> Yaml {
    Yaml::Map(
        items
            .iter()
            .map(|(key, value)| {
                let yaml = match value {
                    Value::Text(text) => Yaml::Str(text.clone()),
                    Value::Expr(text) => Yaml::Expr(text.clone()),
                    Value::Int(number) => Yaml::Int(*number),
                    Value::Bool(flag) => Yaml::Bool(*flag),
                };
                entry(key, yaml)
            })
            .collect(),
    )
}
