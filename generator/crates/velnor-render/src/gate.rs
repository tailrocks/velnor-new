// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{Gate, GateNeed, Job, PlanFrom, RootSlug, When};

const SETUP_DONE: &str = "steps.restored.outcome == 'success'";
pub(crate) const PLAN_STEP: &str = "plan";
const NOT_CANCELLED: &str = "!cancelled()";
const NOT_DRAFT: &str = "github.event.pull_request.draft != true";
const SCHEDULE: &str = "schedule";
const PUSH: &str = "push";

pub(crate) fn output(slug: &RootSlug, need: GateNeed) -> String {
    if need.is_global() {
        format!("rust_{}", need_name(need))
    } else {
        format!("rust_{slug}_{}", need_name(need))
    }
}

pub(crate) fn plan_source(from: &PlanFrom) -> String {
    match from {
        PlanFrom::ThisJob => format!("steps.{PLAN_STEP}.outputs"),
        PlanFrom::Job(job) => format!("needs.{job}.outputs"),
    }
}

fn need_name(need: GateNeed) -> &'static str {
    match need {
        GateNeed::Selected => "mode",
        GateNeed::Doctests => "doctests",
        GateNeed::DoctestsKnown => "doctests_known",
        GateNeed::Docs => "docs",
        GateNeed::TestFilter => "test_filter",
        GateNeed::PackageArgs => "package_args",
        GateNeed::Deny => "deny",
        GateNeed::Machete => "machete",
        GateNeed::Policy => "policy",
    }
}

pub(crate) fn condition(
    when: When,
    gate: Option<&Gate>,
    after_setup: bool,
    skip_draft: bool,
) -> Option<String> {
    wrapped(&condition_parts(when, gate, after_setup, skip_draft))
}

pub(crate) fn job_condition(job: &Job) -> Option<String> {
    let mut parts = condition_parts(job.when, job.gate.as_ref(), false, job.skip_on_draft);
    let alternatives: Option<Vec<String>> = job
        .any_of
        .iter()
        .map(|run| {
            let parts = condition_parts(run.when, run.gate.as_ref(), false, run.skip_on_draft);
            (!parts.is_empty()).then(|| format!("({})", parts.join(" && ")))
        })
        .collect();
    if let Some(mut alternatives) = alternatives {
        let mut seen = BTreeSet::new();
        alternatives.retain(|alternative| seen.insert(alternative.clone()));
        if !alternatives.is_empty() {
            parts.push(format!("({})", alternatives.join(" || ")));
        }
    }
    wrapped(&parts)
}

fn wrapped(parts: &[String]) -> Option<String> {
    if parts.is_empty() {
        None
    } else {
        Some(format!("${{{{ {} }}}}", parts.join(" && ")))
    }
}

fn condition_parts(
    when: When,
    gate: Option<&Gate>,
    after_setup: bool,
    skip_draft: bool,
) -> Vec<String> {
    let mut parts = Vec::new();
    match when {
        When::Success => {}
        When::NotCancelled => {
            parts.push(NOT_CANCELLED.to_owned());
            if after_setup {
                parts.push(SETUP_DONE.to_owned());
            }
        }
        When::Scheduled => parts.push(format!("github.event_name == '{SCHEDULE}'")),
        When::ScheduledOrPush => parts.push(format!(
            "(github.event_name == '{SCHEDULE}' || github.event_name == '{PUSH}')"
        )),
    }
    if skip_draft {
        parts.push(NOT_DRAFT.to_owned());
    }
    if let Some(gate) = gate {
        parts.extend(gate_parts(gate));
    }
    parts
}

fn gate_parts(gate: &Gate) -> Vec<String> {
    let source = plan_source(&gate.from);
    let mut parts = match &gate.from {
        PlanFrom::ThisJob => vec![format!("steps.{PLAN_STEP}.outcome == 'success'")],
        PlanFrom::Job(_) => Vec::new(),
    };
    let read = |need| format!("{source}.{}", output(&gate.slug, need));
    let runs = |need| format!("{} == 'run'", read(need));
    match gate.need {
        GateNeed::Selected | GateNeed::Doctests | GateNeed::Docs => {
            parts.push(format!("{} != 'none'", read(GateNeed::Selected)));
            match gate.need {
                GateNeed::Doctests | GateNeed::Docs => {
                    parts.push(runs(gate.need));
                    parts.push(format!("{} != ''", read(GateNeed::PackageArgs)));
                }
                _ => {}
            }
        }
        GateNeed::Deny => parts.push(runs(GateNeed::Deny)),
        GateNeed::Machete => parts.push(runs(GateNeed::Machete)),
        GateNeed::Policy => parts.push(format!(
            "({} || {})",
            runs(GateNeed::Deny),
            runs(GateNeed::Machete)
        )),
        GateNeed::PackageArgs if gate.package.is_some() => {
            parts.extend(gate.package.iter().map(|package| {
                let args = format!("format(' {{0}} ', {})", read(GateNeed::PackageArgs));
                format!("(contains({args}, ' -p {package} ') || contains({args}, ' --workspace '))")
            }));
        }
        GateNeed::TestFilter | GateNeed::PackageArgs | GateNeed::DoctestsKnown => {
            parts.push(format!("{} != ''", read(gate.need)));
        }
    }
    parts
}
