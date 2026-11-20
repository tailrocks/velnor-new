// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Step, StepKind, When};

use crate::gate::{
    BASE_STEP, ON_DEFAULT_BRANCH, Setup, all_conditions, condition, gate_parts, pr_parts,
};

const NOT_CANCELLED: &str = "!cancelled()";
const INPUTS_SAVED: &str = "steps.inputs_save.outcome == 'success'";

pub(crate) fn combined(
    chained: bool,
    step: &Step,
    setup: Setup,
    extra: &[String],
    saved_after: &[String],
    first: Option<String>,
) -> Option<String> {
    if chained {
        if let Some(decision) = decision(step, extra, first.as_deref()) {
            return Some(decision);
        }
        if let Some(save) = after_decision(step, setup, extra, saved_after) {
            return all_conditions(first, Some(save));
        }
    }
    all_conditions(
        first,
        condition(
            step.when,
            step.gate.as_ref(),
            setup,
            step.pr,
            extra,
            saved_after,
        ),
    )
}

fn decision(step: &Step, extra: &[String], first: Option<&str>) -> Option<String> {
    let follows = first == Some("${{ steps.inputs_save.outcome == 'success' }}");
    (matches!(step.kind, StepKind::CacheDecide { .. })
        && step.when == When::SaveBase
        && step.pr == velnor_model::PrRun::Always
        && extra.is_empty()
        && follows)
        .then(|| format!("${{{{ {NOT_CANCELLED} && {INPUTS_SAVED} }}}}"))
}

fn after_decision(
    step: &Step,
    setup: Setup,
    extra: &[String],
    saved_after: &[String],
) -> Option<String> {
    let output = match step.when {
        When::SaveBaseDecided => "save",
        When::SaveSources => "sources",
        _ => return None,
    };
    let mut parts = vec![
        NOT_CANCELLED.to_owned(),
        format!("steps.{BASE_STEP}.outputs.{output} == 'true'"),
        ON_DEFAULT_BRANCH.to_owned(),
    ];
    parts.extend(saved_after.is_empty().then(|| "success()".to_owned()));
    parts.extend(pr_parts(step.pr));
    parts.extend(extra.iter().cloned());
    if let Some(gate) = &step.gate {
        parts.extend(gate_parts(gate, setup == Setup::Replanned));
    }
    Some(format!("${{{{ {} }}}}", parts.join(" && ")))
}
