// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Job, LintCommand, LintJob, LintKind, RepoFacts, Step, StepKind};

use crate::config_lint::{JACTIONLINT, SHELLCHECK, tool_key};
use crate::plan::{CHECKOUT_DEPTH, job, step};

pub(crate) fn lint_jobs(facts: &RepoFacts) -> Vec<Job> {
    facts
        .config
        .lint
        .iter()
        .map(|lint| lint_job(facts, lint))
        .collect()
}

fn resolved(facts: &RepoFacts, name: &str) -> String {
    tool_key(&facts.tools, name).unwrap_or(name).to_owned()
}

fn install(facts: &RepoFacts, lint: &LintJob) -> Vec<Step> {
    match &lint.kind {
        LintKind::Jactionlint { .. } => {
            let mut tools = vec![resolved(facts, JACTIONLINT)];
            tools.extend(shellcheck(facts).then(|| resolved(facts, SHELLCHECK)));
            vec![step(StepKind::InstallTools {
                only: Some(tools.join(" ")),
            })]
        }
        LintKind::Alint { .. } => Vec::new(),
        LintKind::MiseTask { tools, .. } if tools.is_empty() => {
            vec![step(StepKind::InstallMise)]
        }
        LintKind::MiseTask { tools, .. } => vec![step(StepKind::InstallTools {
            only: Some(
                tools
                    .iter()
                    .map(|tool| resolved(facts, tool))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        })],
    }
}

fn shellcheck(facts: &RepoFacts) -> bool {
    tool_key(&facts.tools, SHELLCHECK).is_some()
}

fn commands(facts: &RepoFacts, lint: &LintJob) -> Vec<LintCommand> {
    match &lint.kind {
        LintKind::Jactionlint { config } => vec![LintCommand::Jactionlint {
            config: config.clone(),
            shellcheck: shellcheck(facts),
        }],
        LintKind::Alint { config, probe } => std::iter::once(LintCommand::Alint {
            config: config.clone(),
        })
        .chain(probe.iter().cloned().map(LintCommand::Probe))
        .collect(),
        LintKind::MiseTask { task, .. } => vec![LintCommand::MiseTask(task.clone())],
    }
}

fn lint_job(facts: &RepoFacts, lint: &LintJob) -> Job {
    let mut steps = vec![step(StepKind::Checkout {
        depth: CHECKOUT_DEPTH,
        sparse: None,
    })];
    steps.extend(install(facts, lint));
    steps.extend(
        commands(facts, lint)
            .into_iter()
            .map(|command| step(StepKind::Lint(command))),
    );
    Job {
        name: Some(lint.name.clone()),
        timeout_minutes: lint.timeout_minutes,
        steps,
        ..job(facts, lint.id.clone())
    }
}
