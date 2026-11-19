// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_release::{BinaryPlan, CheckPlan, CheckStage, ReleasePlan};

use crate::release::{FROM_A_PASSED_PUSH, GATE_IS_READY, HOSTED_RUNNER, VERIFIED_COMMIT};
use crate::release_binaries::{build_id, download};
use crate::release_steps::{checkout, expression, permissions};
use crate::rules::{RenderedJob, RenderedStep};

pub(crate) fn tag_stage(plan: &ReleasePlan) -> Vec<RenderedJob> {
    stage_jobs(plan, CheckStage::BeforeTag)
}

pub(crate) fn publish_stage(plan: &ReleasePlan) -> Vec<RenderedJob> {
    stage_jobs(plan, CheckStage::BeforePublish)
}

pub(crate) fn tag_needs(plan: &ReleasePlan) -> Vec<String> {
    plan.checks
        .iter()
        .filter(|check| check.stage == CheckStage::BeforeTag)
        .map(|check| check.id.clone())
        .collect()
}

pub(crate) fn publish_needs(plan: &ReleasePlan, binary: &BinaryPlan) -> Vec<String> {
    plan.checks
        .iter()
        .filter(|check| {
            check
                .artifacts
                .iter()
                .any(|artifact| artifact.package == binary.package)
        })
        .map(|check| check.id.clone())
        .collect()
}

fn stage_jobs(plan: &ReleasePlan, stage: CheckStage) -> Vec<RenderedJob> {
    plan.checks
        .iter()
        .filter(|check| check.stage == stage)
        .map(job)
        .collect()
}

fn job(check: &CheckPlan) -> RenderedJob {
    let mut needs = vec!["gate".to_owned()];
    let mut conditions = vec![GATE_IS_READY.to_owned(), FROM_A_PASSED_PUSH.to_owned()];
    let mut steps = vec![checkout(VERIFIED_COMMIT, 1)];
    if check.stage == CheckStage::BeforePublish {
        needs.push("tag".to_owned());
        let packages: BTreeSet<&str> = check
            .artifacts
            .iter()
            .map(|artifact| artifact.package.as_str())
            .collect();
        for package in packages {
            conditions.push(format!("needs.tag.outputs.{package} != ''"));
        }
        for artifact in &check.artifacts {
            needs.push(build_id(&artifact.package, artifact.platform));
            steps.push(download(crate::release_binaries::artifact_name(
                &artifact.package,
                artifact.platform,
            )));
        }
    }
    needs.extend(check.needs.iter().cloned());
    steps.push(RenderedStep {
        name: check.name.clone(),
        run: Some(check.run.clone()),
        ..RenderedStep::default()
    });
    let scopes: Vec<(&str, &str)> = check
        .permissions
        .iter()
        .map(|(scope, access)| (scope.as_str(), access.as_str()))
        .collect();
    RenderedJob {
        id: check.id.clone(),
        name: Some(check.name.clone()),
        needs,
        condition: Some(expression(&conditions.join(" && "))),
        permissions: permissions(&scopes),
        runs_on: check
            .runs_on
            .clone()
            .unwrap_or_else(|| HOSTED_RUNNER.to_owned()),
        timeout_minutes: check.timeout_minutes,
        steps,
        ..RenderedJob::default()
    }
}
