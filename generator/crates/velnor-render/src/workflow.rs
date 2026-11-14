// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedJob, RenderedStep, Value};
use crate::yaml::{Yaml, entry, text};

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
    items.extend([
        entry("runs-on", text(&job.runs_on)),
        entry("timeout-minutes", Yaml::Int(u64::from(job.timeout_minutes))),
        entry(
            "steps",
            Yaml::Seq(job.steps.iter().map(yaml_step).collect()),
        ),
    ]);
    match &job.comment {
        Some(comment) => Yaml::Noted(comment.clone(), Box::new(Yaml::Map(items))),
        None => Yaml::Map(items),
    }
}

fn yaml_step(step: &RenderedStep) -> Yaml {
    let mut items = vec![entry("name", text(&step.name))];
    if let Some(condition) = &step.condition {
        items.push(entry("if", Yaml::Expr(condition.clone())));
    }
    if let Some(uses) = &step.uses {
        items.push(entry("uses", text(uses)));
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

fn values(items: &[(String, Value)]) -> Yaml {
    Yaml::Map(
        items
            .iter()
            .map(|(key, value)| {
                let yaml = match value {
                    Value::Text(text) => Yaml::Str(text.clone()),
                    Value::Expr(text) => Yaml::Expr(text.clone()),
                    Value::Selected(text) => Yaml::Selected(text.clone()),
                    Value::Int(number) => Yaml::Int(*number),
                    Value::Bool(flag) => Yaml::Bool(*flag),
                };
                entry(key, yaml)
            })
            .collect(),
    )
}
