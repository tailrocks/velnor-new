// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::HelperRelease;
use velnor_release::{RELEASE_APP_SLUG, ReleasePlan};

use crate::release::after_the_gate;
use crate::release_steps::{app_token, expression, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep};
use crate::steps::install_helper;

const SETUP_MINUTES: u16 = 5;
const REQUESTS_PER_ASSETLESS_PUBLISH: u16 = 5;
const MINUTES_PER_REQUEST: u16 = 5;
const TOKEN_BOUND_MINUTES: u16 = 50;
const ASSETLESS_PUBLISH_HELPER: (u64, u64, u64) = (0, 1, 81);

#[must_use]
pub fn unfinalized_without_binary(plan: &ReleasePlan, helper: &HelperRelease) -> BTreeSet<String> {
    if helper.version().at_least(ASSETLESS_PUBLISH_HELPER) {
        return BTreeSet::new();
    }
    assetless(plan)
}

pub(crate) fn without_binary(plan: &ReleasePlan, helper: &HelperRelease) -> BTreeSet<String> {
    if !helper.version().at_least(ASSETLESS_PUBLISH_HELPER) {
        return BTreeSet::new();
    }
    assetless(plan)
}

fn assetless(plan: &ReleasePlan) -> BTreeSet<String> {
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
    steps.extend(
        packages
            .iter()
            .enumerate()
            .map(|(index, package)| publish(index, package)),
    );
    steps.push(fail_when_a_publish_failed(packages.len()));
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
        timeout_minutes: timeout_minutes(packages.len()),
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

fn timeout_minutes(packages: usize) -> u16 {
    let per_package = REQUESTS_PER_ASSETLESS_PUBLISH * MINUTES_PER_REQUEST;
    u16::try_from(packages)
        .unwrap_or(u16::MAX)
        .saturating_mul(per_package)
        .saturating_add(SETUP_MINUTES)
        .min(TOKEN_BOUND_MINUTES)
}

fn publish_id(index: usize) -> String {
    format!("publish_{index}")
}

fn publish(index: usize, package: &str) -> RenderedStep {
    RenderedStep {
        id: Some(publish_id(index)),
        name: format!("Publish the draft release of {package}"),
        condition: Some(expression(&released(package))),
        continue_on_error: true,
        env: variables(&[
            ("GITHUB_TOKEN", "${{ steps.app.outputs.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("TAG", &format!("${{{{ needs.tag.outputs.{package} }}}}")),
        ]),
        run: Some(format!(
            "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app={RELEASE_APP_SLUG} --no-files"
        )),
        ..RenderedStep::default()
    }
}

fn fail_when_a_publish_failed(packages: usize) -> RenderedStep {
    let failed: Vec<String> = (0..packages)
        .map(|index| format!("steps.{}.outcome == 'failure'", publish_id(index)))
        .collect();
    RenderedStep {
        name: "Fail when a draft release was not published".to_owned(),
        condition: Some(expression(&failed.join(" || "))),
        run: Some("exit 1".to_owned()),
        ..RenderedStep::default()
    }
}
