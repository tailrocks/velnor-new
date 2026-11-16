// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleasePlan, RootPlan};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, tag_id};
use crate::release_steps::{expression, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep};

const FINALIZE_TIMEOUT_MINUTES: u16 = 10;

pub(crate) fn without_binary(plan: &ReleasePlan, root: &RootPlan) -> Vec<String> {
    root.packages
        .iter()
        .map(|(package, _)| package)
        .chain(&root.inherited)
        .filter(|package| {
            plan.binaries
                .iter()
                .all(|binary| binary.package != **package)
        })
        .cloned()
        .collect()
}

pub(crate) fn finalize_jobs(plan: &ReleasePlan) -> Vec<RenderedJob> {
    plan.roots
        .iter()
        .enumerate()
        .filter_map(|(index, root)| {
            let packages = without_binary(plan, root);
            (!packages.is_empty()).then(|| finalize(index, &packages))
        })
        .collect()
}

fn finalize(index: usize, packages: &[String]) -> RenderedJob {
    let tag = tag_id(index);
    RenderedJob {
        id: format!("finalize-{index}"),
        needs: vec!["gate".to_owned(), tag.clone()],
        condition: Some(expression(&format!(
            "{GATE_IS_READY} && {FROM_A_PASSED_PUSH}"
        ))),
        permissions: permissions(&[("contents", "write")]),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: FINALIZE_TIMEOUT_MINUTES,
        steps: packages
            .iter()
            .map(|package| RenderedStep {
                name: format!("Publish the draft release of {package}"),
                condition: Some(expression(&format!("needs.{tag}.outputs.{package} != ''"))),
                env: variables(&[
                    ("GH_TOKEN", "${{ github.token }}"),
                    ("REPOSITORY", "${{ github.repository }}"),
                    ("TAG", &format!("${{{{ needs.{tag}.outputs.{package} }}}}")),
                ]),
                run: Some(
                    "gh release edit \"$TAG\" --repo \"$REPOSITORY\" --draft=false".to_owned(),
                ),
                ..RenderedStep::default()
            })
            .collect(),
        ..RenderedJob::default()
    }
}
