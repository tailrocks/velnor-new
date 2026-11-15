// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Gate, GateNeed, PlanFrom, When};

const SETUP_DONE: &str = "steps.restored.outcome == 'success'";
pub(crate) const PLAN_STEP: &str = "plan";
const NOT_CANCELLED: &str = "!cancelled()";
const NOT_DRAFT: &str = "github.event.pull_request.draft != true";

pub(crate) fn output(slug: &str, need: GateNeed) -> String {
    format!("rust_{slug}_{}", need_name(need))
}

fn need_name(need: GateNeed) -> &'static str {
    match need {
        GateNeed::Selected => "mode",
        GateNeed::Doctests => "doctests",
        GateNeed::Docs => "docs",
    }
}

pub(crate) fn condition(
    when: When,
    gate: Option<&Gate>,
    after_setup: bool,
    skip_draft: bool,
) -> Option<String> {
    let mut parts = Vec::new();
    if when == When::NotCancelled {
        parts.push(NOT_CANCELLED.to_owned());
        if after_setup {
            parts.push(SETUP_DONE.to_owned());
        }
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
    let (source, mut parts) = match &gate.from {
        PlanFrom::ThisJob => (
            format!("steps.{PLAN_STEP}.outputs"),
            vec![format!("steps.{PLAN_STEP}.outcome == 'success'")],
        ),
        PlanFrom::Job(job) => (format!("needs.{job}.outputs"), Vec::new()),
    };
    let read = |need| format!("{source}.{}", output(&gate.slug, need));
    parts.push(format!("{} != 'none'", read(GateNeed::Selected)));
    if gate.need != GateNeed::Selected {
        parts.push(format!("{} == 'run'", read(gate.need)));
    }
    parts
}
