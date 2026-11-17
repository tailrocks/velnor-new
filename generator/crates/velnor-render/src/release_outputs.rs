// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::HelperRelease;
use velnor_release::ReleasePlan;

use crate::release_steps::{COMMIT_OUTPUT_SUFFIX, TAGGED_COMMIT_HELPER};
use crate::rules::{Rule, RuleViolation};

fn recorded_outputs(plan: &ReleasePlan, helper: &HelperRelease) -> Vec<String> {
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
    let commits = helper.version().at_least(TAGGED_COMMIT_HELPER);
    packages
        .into_iter()
        .flat_map(|package| {
            std::iter::once(package.clone()).chain(commits.then(|| commit_output(package)))
        })
        .collect()
}

pub(crate) fn output_collisions(
    plan: &ReleasePlan,
    helper: &HelperRelease,
    job: &str,
) -> Vec<RuleViolation> {
    let mut seen = BTreeSet::new();
    recorded_outputs(plan, helper)
        .into_iter()
        .filter(|name| !seen.insert(name.clone()))
        .map(|name| RuleViolation {
            job: job.to_owned(),
            step: 0,
            rule: Rule::OutputNameCollision(name),
        })
        .collect()
}

pub(crate) fn commit_outputs(plan: &ReleasePlan, helper: &HelperRelease) -> Vec<(String, String)> {
    if !helper.version().at_least(TAGGED_COMMIT_HELPER) {
        return Vec::new();
    }
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
