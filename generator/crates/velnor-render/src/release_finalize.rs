// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::HelperRelease;
use velnor_release::{RELEASE_APP_SLUG, ReleasePlan};

use crate::release::after_the_gate;
use crate::release_steps::{app_token, expression, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep};
use crate::steps::install_helper;

const FINALIZE_TIMEOUT_MINUTES: u16 = 10;
const ASSETLESS_PUBLISH_HELPER: (u64, u64, u64) = (0, 1, 81);

pub(crate) fn without_binary(plan: &ReleasePlan, helper: &HelperRelease) -> BTreeSet<String> {
    if !helper.version().at_least(ASSETLESS_PUBLISH_HELPER) {
        return BTreeSet::new();
    }
    plan.roots
        .iter()
        .flat_map(|root| {
            root.packages
                .iter()
                .map(|(package, _)| package)
                .chain(&root.inherited)
        })
        .filter(|package| {
            plan.binaries
                .iter()
                .all(|binary| binary.package != **package)
        })
        .cloned()
        .collect()
}

pub(crate) fn finalize_job(plan: &ReleasePlan, helper: &HelperRelease) -> Option<RenderedJob> {
    let packages = without_binary(plan, helper);
    if packages.is_empty() {
        return None;
    }
    let released: Vec<String> = packages.iter().map(|package| released(package)).collect();
    let mut steps = vec![install_helper(helper), app_token(&[("contents", "write")])];
    steps.extend(packages.iter().map(|package| publish(package)));
    let gated = after_the_gate("finalize");
    Some(RenderedJob {
        needs: vec!["gate".to_owned(), "tag".to_owned()],
        condition: gated.condition.map(|gate| {
            let gate = gate
                .strip_prefix("${{ ")
                .and_then(|text| text.strip_suffix(" }}"))
                .unwrap_or(&gate)
                .to_owned();
            expression(&format!("{gate} && ({})", released.join(" || ")))
        }),
        permissions: permissions(&[("contents", "read")]),
        timeout_minutes: FINALIZE_TIMEOUT_MINUTES,
        steps,
        id: gated.id,
        environment: gated.environment,
        runs_on: gated.runs_on,
        ..RenderedJob::default()
    })
}

fn released(package: &str) -> String {
    format!("needs.tag.outputs.{package} != ''")
}

fn publish(package: &str) -> RenderedStep {
    RenderedStep {
        name: format!("Publish the draft release of {package}"),
        condition: Some(expression(&released(package))),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ steps.app.outputs.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("TAG", &format!("${{{{ needs.tag.outputs.{package} }}}}")),
        ]),
        run: Some(format!(
            "velnor-ci release publish --repo \"$REPOSITORY\" --tag \"$TAG\" --app {RELEASE_APP_SLUG} --no-files"
        )),
        ..RenderedStep::default()
    }
}
