// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{
    RenderedConcurrency, RenderedContainer, RenderedJob, RenderedMatrix, RenderedMatrixSource,
    RenderedStep, Value,
};
use crate::yaml::{Yaml, entry, text};
use velnor_model::{
    MERGE_GROUP_SKIP_OUTPUT, TESTS_BY_PACKAGE_OUTPUT, TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT,
    TESTS_RUN_OUTPUT,
};

/// True for every event except a pull request from a fork.
pub(crate) const SAME_REPOSITORY: &str = "(!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository)";

pub const TRUSTED_EVENT: &str = "(github.event_name == 'schedule' || ((github.event_name == 'push' || github.event_name == 'workflow_dispatch') && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)))";

/// Where the generated workflow tells jactionlint that `ShellCheck` cannot follow
/// the launcher: on each launcher step, or, when that would push the file past
/// the size GitHub accepts, once on each job that has one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellcheckMarks {
    Steps,
    Jobs,
}

pub(crate) fn yaml_job(job: &RenderedJob, marks: ShellcheckMarks) -> Yaml {
    let mut items = Vec::new();
    if let Some(expression) = &job.check_name {
        items.push(entry("name", Yaml::Expr(expression.clone())));
    } else if let Some(name) = &job.name {
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
        let value = text(environment);
        items.push(entry(
            "environment",
            match &job.environment_note {
                Some(note) => Yaml::Noted(note.clone(), Box::new(value)),
                None => value,
            },
        ));
    }
    if job.no_permissions {
        items.push(entry("permissions", Yaml::Map(Vec::new())));
    } else if !job.permissions.is_empty() {
        items.push(entry("permissions", pairs(&job.permissions)));
    }
    if let Some(concurrency) = &job.concurrency {
        items.push(entry("concurrency", yaml_concurrency(concurrency)));
    }
    if let Some(mode) = cache_mode(job) {
        items.push(entry("cache-mode", text(mode)));
    }
    if !job.outputs.is_empty() {
        items.push(entry("outputs", yaml_outputs(&job.outputs)));
    }
    if job.uses.is_none() {
        items.push(entry("runs-on", runs_on(job)));
    }
    if let Some(container) = &job.container {
        items.push(entry("container", yaml_container(container)));
    }
    if !job.services.is_empty() {
        items.push(entry(
            "services",
            Yaml::Map(
                job.services
                    .iter()
                    .map(|(name, service)| entry(name, yaml_container(service)))
                    .collect(),
            ),
        ));
    }
    if let Some(matrix) = &job.matrix {
        items.push(entry("strategy", yaml_matrix(matrix)));
    }
    if let Some(uses) = &job.uses {
        items.push(entry("uses", text(uses)));
        if !job.with.is_empty() {
            items.push(entry("with", values(&job.with)));
        }
    } else {
        items.push(entry(
            "timeout-minutes",
            Yaml::Int(u64::from(job.timeout_minutes)),
        ));
        if !job.env.is_empty() {
            items.push(entry("env", values(&job.env)));
        }
        items.extend([entry(
            "steps",
            Yaml::Seq(
                job.steps
                    .iter()
                    .map(|step| yaml_step(step, marks))
                    .collect(),
            ),
        )]);
    }
    match job_notes(job, marks) {
        Some(notes) => Yaml::Noted(notes, Box::new(Yaml::Map(items))),
        None => Yaml::Map(items),
    }
}

/// The `cache-mode` of a job: the declared one, else `read`. A job with no key gets GitHub's
/// default, which is `write` on a trusted trigger, so the absence is never rendered. A job that
/// calls a reusable workflow (`uses`) has no key of its own: its callee jobs carry theirs.
fn cache_mode(job: &RenderedJob) -> Option<&str> {
    match (&job.cache_mode, &job.uses) {
        (Some(mode), _) => Some(mode),
        (None, None) => Some("read"),
        (None, Some(_)) => None,
    }
}

pub(crate) fn yaml_concurrency(concurrency: &RenderedConcurrency) -> Yaml {
    Yaml::Map(vec![
        entry("group", Yaml::Expr(concurrency.group.clone())),
        entry(
            "cancel-in-progress",
            Yaml::Bool(concurrency.cancel_in_progress),
        ),
    ])
}

fn yaml_outputs(outputs: &[(String, String)]) -> Yaml {
    Yaml::Map(
        outputs
            .iter()
            .map(|(name, value)| {
                let value = value.clone();
                entry(
                    name,
                    if name == TESTS_RUN_OUTPUT
                        || name == MERGE_GROUP_SKIP_OUTPUT
                        || name == TESTS_LISTED_OUTPUT
                        || name == TESTS_IGNORED_OUTPUT
                        || name == TESTS_BY_PACKAGE_OUTPUT
                    {
                        Yaml::Unread(value)
                    } else {
                        Yaml::Expr(value)
                    },
                )
            })
            .collect(),
    )
}

fn yaml_container(container: &RenderedContainer) -> Yaml {
    let mut items = vec![entry("image", text(&container.image))];
    if !container.env.is_empty() {
        items.push(entry("env", pairs(&container.env)));
    }
    if !container.ports.is_empty() {
        items.push(entry(
            "ports",
            Yaml::Seq(container.ports.iter().map(|port| text(port)).collect()),
        ));
    }
    if let Some(options) = &container.options {
        items.push(entry("options", text(options)));
    }
    Yaml::Map(items)
}

fn yaml_matrix(matrix: &RenderedMatrix) -> Yaml {
    let mut items = Vec::new();
    if let Some(fail_fast) = matrix.fail_fast {
        items.push(entry("fail-fast", Yaml::Bool(fail_fast)));
    }
    if let Some(max_parallel) = matrix.max_parallel {
        items.push(entry("max-parallel", Yaml::Int(u64::from(max_parallel))));
    }
    items.push(entry(
        "matrix",
        match &matrix.source {
            RenderedMatrixSource::Output(from) => {
                Yaml::Expr(format!("${{{{ fromJSON({from}) }}}}"))
            }
            RenderedMatrixSource::Axes { axes, include } => Yaml::Map(
                axes.iter()
                    .map(|(name, values)| {
                        entry(
                            name,
                            Yaml::Seq(values.iter().map(|value| text(value)).collect()),
                        )
                    })
                    .chain((!include.is_empty()).then(|| {
                        entry(
                            "include",
                            Yaml::Seq(include.iter().map(|item| pairs(item)).collect()),
                        )
                    }))
                    .collect(),
            ),
        },
    ));
    Yaml::Map(items)
}

fn job_notes(job: &RenderedJob, marks: ShellcheckMarks) -> Option<String> {
    let launches_helper = marks == ShellcheckMarks::Jobs && job.steps.iter().any(launches_helper);
    let notes: Vec<&str> = job
        .comment
        .as_deref()
        .into_iter()
        .chain(launches_helper.then_some(crate::helper_integrity::SHELLCHECK_IGNORE))
        .collect();
    (!notes.is_empty()).then(|| notes.join("\n"))
}

fn launches_helper(step: &RenderedStep) -> bool {
    step.run
        .as_deref()
        .is_some_and(|run| run.starts_with(crate::helper_integrity::INVOCATION_PREFIX))
}

fn runs_on(job: &RenderedJob) -> Yaml {
    let primary = match &job.runs_on_axis {
        Some(axis) => format!("matrix.{axis}"),
        None => literal(&job.runs_on),
    };
    let fenced = match &job.fork_runner {
        Some(fork) => format!("{SAME_REPOSITORY} && {primary} || {}", literal(fork)),
        None => primary.clone(),
    };
    match (&job.trusted_runner, &job.fork_runner, &job.runs_on_axis) {
        (Some(trusted), fork, axis) => {
            let label = literal(&trusted.label);
            let pick = match axis {
                Some(_) => format!(
                    "{TRUSTED_EVENT} && {primary} == {} && {label}",
                    literal(&trusted.replaces)
                ),
                None => format!("{TRUSTED_EVENT} && {label}"),
            };
            let rest = if fork.is_some() {
                format!("({fenced})")
            } else {
                fenced
            };
            Yaml::Expr(format!("${{{{ {pick} || {rest} }}}}"))
        }
        (None, Some(_), _) => Yaml::Expr(format!("${{{{ {fenced} }}}}")),
        (None, None, Some(_)) => Yaml::Expr(format!("${{{{ {primary} }}}}")),
        (None, None, None) => text(&job.runs_on),
    }
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn yaml_step(step: &RenderedStep, marks: ShellcheckMarks) -> Yaml {
    let mut items = Vec::new();
    if let Some(id) = &step.id {
        items.push(entry("id", text(id)));
    }
    items.push(entry("name", text(&step.name)));
    if let Some(condition) = &step.condition {
        items.push(entry("if", Yaml::Expr(condition.clone())));
    }
    if step.continue_on_error {
        items.push(entry("continue-on-error", Yaml::Bool(true)));
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
        let value = if directory.contains("${{") {
            Yaml::Expr(directory.clone())
        } else {
            text(directory)
        };
        items.push(entry("working-directory", value));
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
    let rendered = Yaml::Map(items);
    if marks == ShellcheckMarks::Steps && launches_helper(step) {
        Yaml::Noted(
            crate::helper_integrity::SHELLCHECK_IGNORE.to_owned(),
            Box::new(rendered),
        )
    } else {
        rendered
    }
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
                    Value::Expr(text) | Value::BoundedExpr { text, .. } => Yaml::Expr(text.clone()),
                    Value::Int(number) => Yaml::Int(*number),
                    Value::Bool(flag) => Yaml::Bool(*flag),
                };
                let yaml = if (key.starts_with("VELNOR_HELPER") || key == "SOURCES_LOCKS") && matches!(value, Value::Text(text) | Value::Expr(text) if text.len() > 100 && !text.contains('\n')) {
                    Yaml::Shared(Box::new(yaml))
                } else {
                    yaml
                };
                entry(key, yaml)
            })
            .collect(),
    )
}
