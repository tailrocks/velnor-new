// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Gate, GateNeed, Job, LintCommand, LintJob, LintKind, PlanFrom, RepoFacts, Step, StepKind,
};

use crate::lane::PLAN_JOB;
use crate::lint_tools::{ALINT, JACTIONLINT, NODE, SHELLCHECK, usable};
use crate::plan::{CHECKOUT_DEPTH, job, quick, step};

pub(crate) fn lint_jobs(facts: &RepoFacts) -> Vec<Job> {
    facts
        .config
        .lint
        .iter()
        .map(|lint| lint_job(facts, lint))
        .collect()
}

fn gate(lint: &LintJob, from: PlanFrom) -> Gate {
    Gate {
        slug: lint.slug(),
        need: GateNeed::Lint,
        from,
        packages: Vec::new(),
        unselected: false,
        group: None,
    }
}

fn resolved(facts: &RepoFacts, name: &str) -> String {
    usable(&facts.tools, &facts.config.runner, name)
        .unwrap_or(name)
        .to_owned()
}

fn install(facts: &RepoFacts, lint: &LintJob) -> Vec<Step> {
    let only = |names: Vec<String>| {
        vec![step(StepKind::InstallTools {
            only: Some(names.join(" ")),
        })]
    };
    match &lint.kind {
        LintKind::Jactionlint { .. } => {
            let mut tools = vec![resolved(facts, JACTIONLINT)];
            tools.extend(available(facts, SHELLCHECK).then(|| resolved(facts, SHELLCHECK)));
            tools.extend(available(facts, NODE).then(|| resolved(facts, NODE)));
            only(tools)
        }
        LintKind::Alint { .. } => only(vec![resolved(facts, ALINT)]),
        LintKind::MiseTask { tools, .. } if tools.is_empty() => {
            vec![step(StepKind::InstallMise)]
        }
        LintKind::MiseTask { tools, .. } => {
            only(tools.iter().map(|tool| resolved(facts, tool)).collect())
        }
    }
}

fn available(facts: &RepoFacts, name: &str) -> bool {
    usable(&facts.tools, &facts.config.runner, name).is_some()
}

fn commands(facts: &RepoFacts, lint: &LintJob) -> Vec<LintCommand> {
    match &lint.kind {
        LintKind::Jactionlint { config } => vec![LintCommand::Jactionlint {
            config: config.clone(),
            shellcheck: available(facts, SHELLCHECK),
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
    let planned = !lint.inputs.is_empty();
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
        pr: quick(facts),
        needs: if planned { vec![PLAN_JOB] } else { Vec::new() },
        gate: planned.then(|| gate(lint, PlanFrom::Job(PLAN_JOB))),
        steps,
        ..job(facts, lint.id.clone())
    }
}
