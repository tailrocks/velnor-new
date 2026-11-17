// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_release::ReleasePlan;

use crate::release_steps::COMMIT_OUTPUT_SUFFIX;
use crate::rules::{Rule, RuleViolation};

fn recorded_outputs(plan: &ReleasePlan) -> Vec<String> {
    let packages: BTreeSet<&String> = plan
        .roots
        .iter()
        .flat_map(|root| {
            root.packages
                .iter()
                .map(|(package, _)| package)
                .chain(&root.inherited)
        })
        .chain(plan.binaries.iter().map(|binary| &binary.package))
        .collect();
    packages
        .into_iter()
        .flat_map(|package| [package.clone(), commit_output(package)])
        .collect()
}

pub(crate) fn output_collisions(plan: &ReleasePlan, job: &str) -> Vec<RuleViolation> {
    let mut seen = BTreeSet::new();
    recorded_outputs(plan)
        .into_iter()
        .filter(|name| !seen.insert(name.clone()))
        .map(|name| RuleViolation {
            job: job.to_owned(),
            step: 0,
            rule: Rule::OutputNameCollision(name),
        })
        .collect()
}

pub(crate) fn commit_outputs(plan: &ReleasePlan) -> Vec<(String, String)> {
    plan.binaries
        .iter()
        .map(|binary| {
            let output = commit_output(&binary.package);
            (
                output.clone(),
                format!("${{{{ steps.record.outputs.{output} }}}}"),
            )
        })
        .collect()
}

pub(crate) fn commit_output(package: &str) -> String {
    format!("{package}{COMMIT_OUTPUT_SUFFIX}")
}
