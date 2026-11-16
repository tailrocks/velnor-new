// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Gate, GateNeed, PlanFrom, RootSlug, When};

const SETUP_DONE: &str = "steps.restored.outcome == 'success'";
pub(crate) const PLAN_STEP: &str = "plan";
const NOT_CANCELLED: &str = "!cancelled()";
const NOT_DRAFT: &str = "github.event.pull_request.draft != true";
const SCHEDULE: &str = "schedule";
const PUSH: &str = "push";

pub(crate) fn output(slug: &RootSlug, need: GateNeed) -> String {
    need.output_name(slug)
}

pub(crate) fn plan_source(from: &PlanFrom) -> String {
    match from {
        PlanFrom::ThisJob => format!("steps.{PLAN_STEP}.outputs"),
        PlanFrom::Job(job) => format!("needs.{job}.outputs"),
    }
}

pub(crate) fn condition(
    when: When,
    gate: Option<&Gate>,
    after_setup: bool,
    skip_draft: bool,
) -> Option<String> {
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
    if parts.is_empty() {
        None
    } else {
        Some(format!("${{{{ {} }}}}", parts.join(" && ")))
    }
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
        GateNeed::TestFilter
        | GateNeed::PackageArgs
        | GateNeed::DoctestsKnown
        | GateNeed::TestsExpected => {
            parts.push(format!("{} != ''", read(gate.need)));
        }
    }
    parts
}
