// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Gate, GateNeed, PlanFrom, When};

pub(crate) const PLAN_STEP: &str = "plan";
const NOT_CANCELLED: &str = "!cancelled()";

pub(crate) fn output(slug: &str, name: &str) -> String {
    format!("rust_{slug}_{name}")
}

pub(crate) fn condition(when: When, gate: Option<&Gate>) -> Option<String> {
    let mut parts = Vec::new();
    if when == When::NotCancelled {
        parts.push(NOT_CANCELLED.to_owned());
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
    let read = |name: &str| format!("{source}.{}", output(&gate.slug, name));
    parts.push(format!("{} != 'none'", read("mode")));
    match gate.need {
        GateNeed::Selected => {}
        GateNeed::Doctests => parts.push(format!("{} == 'run'", read("doctests"))),
        GateNeed::Docs => parts.push(format!("{} == 'run'", read("docs"))),
    }
    parts
}
