// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{Gate, GateNeed, Job, PlanFrom, package_args_env};

use crate::gate::{REPLAN_STEP, output};
use crate::rules::Value;

pub(crate) fn package_selection(
    gate: &Gate,
    package_args: &str,
    replanned: bool,
) -> Option<String> {
    if gate.packages.is_empty() {
        return None;
    }
    let (args, replan) = match gate.from {
        PlanFrom::ThisJob => (format!("format(' {{0}} ', {package_args})"), None),
        PlanFrom::Job(_) | PlanFrom::WorkJob(_) => (
            format!("env.{}", package_args_env(&gate.slug)),
            replanned.then(|| format!("steps.{REPLAN_STEP}.outcome == 'success'")),
        ),
    };
    let selected = replan
        .into_iter()
        .chain(
            gate.packages
                .iter()
                .map(|package| format!("contains({args}, ' -p {package} ')")),
        )
        .chain([format!("contains({args}, ' --workspace ')")])
        .collect::<Vec<_>>()
        .join(" || ");
    Some(if gate.unselected {
        format!("!({selected})")
    } else {
        format!("({selected})")
    })
}

pub(crate) fn job_env(job: &Job) -> Vec<(String, Value)> {
    let reads: BTreeMap<String, String> = job
        .gate
        .iter()
        .chain(
            job.steps
                .iter()
                .flat_map(|step| step.gate.iter().chain(&step.any_gate)),
        )
        .filter(|gate| !gate.packages.is_empty())
        .filter_map(|gate| match &gate.from {
            PlanFrom::ThisJob => None,
            PlanFrom::Job(from) | PlanFrom::WorkJob(from) => {
                let key = output(&gate.slug, GateNeed::PackageArgs);
                Some((
                    package_args_env(&gate.slug),
                    format!("[ ${{{{ needs.{from}.outputs.{key} }}}} ]"),
                ))
            }
        })
        .collect();
    reads
        .into_iter()
        .map(|(name, value)| (name, Value::Expr(value)))
        .collect()
}
