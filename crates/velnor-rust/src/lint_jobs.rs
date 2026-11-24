// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    Gate, GateNeed, Job, LintCommand, LintJob, LintKind, PlanFrom, PrRun, RepoFacts, Step, StepKind,
};

use crate::detect_checks::generator_root;
use crate::lane::PLAN_JOB;
use crate::lint_tools::{ALINT, JACTIONLINT, NODE, RUST, SHELLCHECK, usable};
use crate::mbx_pin::MBX_TOOL;
use crate::plan::{CHECKOUT_DEPTH, job, mbx, quick, step};
use crate::plan_tools::{mirror_step, tools_install, tools_restore};

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
        also: Vec::new(),
    }
}

fn resolved(facts: &RepoFacts, name: &str) -> String {
    usable(&facts.tools, &facts.config.runner, name)
        .unwrap_or(name)
        .to_owned()
}

pub(crate) enum ToolInstall {
    All,
    These(Vec<String>),
}

pub(crate) fn tool_install(facts: &RepoFacts, lint: &LintJob) -> ToolInstall {
    match &lint.kind {
        LintKind::MiseTask { tools, .. } if tools.is_empty() => ToolInstall::All,
        LintKind::Jactionlint { .. } => ToolInstall::These(jactionlint_tools(facts)),
        LintKind::Alint { .. } => ToolInstall::These(vec![resolved(facts, ALINT)]),
        LintKind::MiseTask { tools, .. } => {
            ToolInstall::These(tools.iter().map(|tool| resolved(facts, tool)).collect())
        }
        LintKind::GeneratedCheck { .. } => ToolInstall::These(generated_check_tools(facts)),
    }
}

pub(crate) fn lint_only_tools(facts: &RepoFacts) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for lint in &facts.config.lint {
        match tool_install(facts, lint) {
            ToolInstall::All => return None,
            ToolInstall::These(tools) => names.extend(tools),
        }
    }
    Some(names)
}

fn jactionlint_tools(facts: &RepoFacts) -> Vec<String> {
    let mut tools = vec![resolved(facts, JACTIONLINT)];
    tools.extend(available(facts, SHELLCHECK).then(|| resolved(facts, SHELLCHECK)));
    tools.extend(available(facts, NODE).then(|| resolved(facts, NODE)));
    tools
}

fn install(facts: &RepoFacts, lint: &LintJob) -> Vec<Step> {
    if matches!(lint.kind, LintKind::GeneratedCheck { .. }) {
        return Vec::new();
    }
    match tool_install(facts, lint) {
        ToolInstall::All => vec![step(StepKind::InstallMise)],
        ToolInstall::These(names) if names.is_empty() => Vec::new(),
        ToolInstall::These(names) => vec![step(StepKind::InstallTools {
            only: Some(names.join(" ")),
        })],
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
        LintKind::GeneratedCheck { .. } => Vec::new(),
    }
}

fn generated_check_tools(facts: &RepoFacts) -> Vec<String> {
    available(facts, RUST)
        .then(|| resolved(facts, RUST))
        .into_iter()
        .chain(available(facts, MBX_TOOL).then(|| resolved(facts, MBX_TOOL)))
        .collect()
}

fn generated_check_job(facts: &RepoFacts, lint: &LintJob, from_source: bool) -> Job {
    let root = from_source
        .then(|| generator_root(&facts.rust_roots))
        .flatten();
    let mut steps = vec![step(StepKind::Checkout {
        depth: CHECKOUT_DEPTH,
        sparse: None,
    })];
    if root.is_some() {
        steps.extend(
            facts
                .helper
                .iter()
                .map(|release| step(StepKind::InstallHelper(release.clone()))),
        );
        steps.extend(mbx(facts, None));
        steps.extend(mirror_step(facts));
        steps.extend(tools_restore(facts, None));
        steps.extend(tools_install(facts));
        if !facts.tools.tools.is_empty() {
            steps.extend(mirror_step(facts));
        }
    } else {
        let tools = generated_check_tools(facts);
        if !tools.is_empty() {
            steps.push(step(StepKind::InstallTools {
                only: Some(tools.join(" ")),
            }));
        }
        steps.extend(
            facts
                .helper
                .iter()
                .map(|release| step(StepKind::InstallGenerator(release.clone()))),
        );
    }
    steps.push(Step {
        directory: root.and_then(|root| root.manifest.parent()),
        target_dir: root
            .filter(|root| root.manifest.parent().is_some())
            .map(|root| root.target_dir.clone()),
        ..step(StepKind::GeneratorCheck {
            from_source: root.is_some(),
        })
    });
    Job {
        name: Some(lint.name.clone()),
        timeout_minutes: lint.timeout_minutes,
        pr: if lint.plan_gated {
            quick(facts)
        } else {
            PrRun::Always
        },
        needs: if lint.plan_gated {
            vec![PLAN_JOB]
        } else {
            Vec::new()
        },
        gate: lint.plan_gated.then(|| gate(lint, PlanFrom::Job(PLAN_JOB))),
        steps,
        ..job(facts, lint.id.clone())
    }
}

fn lint_job(facts: &RepoFacts, lint: &LintJob) -> Job {
    if let LintKind::GeneratedCheck { from_source } = lint.kind {
        return generated_check_job(facts, lint, from_source);
    }
    let planned = lint.planned();
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
