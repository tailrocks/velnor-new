// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_release::{BinaryPlan, CheckPlan, CheckStage, ReleasePlan};

use crate::release::{GATE_IS_READY, HOSTED_RUNNER};
use crate::release_assets::published_files;
use crate::release_binaries::{SUMS_FILE, build_id, download};
use crate::release_family::BUILD_OUTPUT;
use crate::release_family_publish::extras;
use crate::release_source::source_of;
use crate::release_steps::{checkout, expression, permissions};
use crate::rules::{RenderedJob, RenderedStep, Rule, RuleViolation};

pub(crate) fn tag_stage(plan: &ReleasePlan) -> Vec<RenderedJob> {
    stage_jobs(plan, CheckStage::BeforeTag)
}

pub(crate) fn publish_stage(plan: &ReleasePlan) -> Vec<RenderedJob> {
    stage_jobs(plan, CheckStage::BeforePublish)
}

pub(crate) fn family_publish_needs(plan: &ReleasePlan) -> Vec<String> {
    plan.checks
        .iter()
        .filter(|check| check.stage == CheckStage::BeforePublish)
        .map(|check| check.id.clone())
        .collect()
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

pub(crate) fn dist_collisions(plan: &ReleasePlan) -> Vec<RuleViolation> {
    let files = |package: &str, platform| -> Vec<String> {
        plan.binaries
            .iter()
            .find(|binary| binary.package == package)
            .map(|binary| published_files(binary, platform))
            .unwrap_or_default()
    };
    let family = plan.family.as_ref().map(|family| {
        let every = plan.binaries.iter().flat_map(|binary| {
            binary
                .platforms
                .iter()
                .map(|platform| (binary.package.as_str(), *platform))
        });
        let reserved = extras(family)
            .into_iter()
            .chain([SUMS_FILE.to_owned()])
            .collect::<Vec<_>>();
        (
            "sums-family".to_owned(),
            every.collect::<Vec<_>>(),
            reserved,
        )
    });
    let checks = plan
        .checks
        .iter()
        .filter(|check| check.stage == CheckStage::BeforePublish)
        .map(|check| {
            let artifacts = check
                .artifacts
                .iter()
                .map(|artifact| (artifact.package.as_str(), artifact.platform))
                .collect::<Vec<_>>();
            (check.id.clone(), artifacts, Vec::new())
        });
    family
        .into_iter()
        .chain(checks)
        .filter_map(|(job, artifacts, reserved)| {
            let mut seen: BTreeSet<String> = reserved.into_iter().collect();
            artifacts
                .into_iter()
                .flat_map(|(package, platform)| {
                    files(package, platform)
                        .into_iter()
                        .collect::<BTreeSet<_>>()
                })
                .find(|file| !seen.insert(file.clone()))
                .map(|file| RuleViolation {
                    job,
                    step: 0,
                    rule: Rule::DistFileCollision(file),
                })
        })
        .collect()
}

fn stage_jobs(plan: &ReleasePlan, stage: CheckStage) -> Vec<RenderedJob> {
    plan.checks
        .iter()
        .filter(|check| check.stage == stage)
        .map(|check| job(plan, check))
        .collect()
}

fn job(plan: &ReleasePlan, check: &CheckPlan) -> RenderedJob {
    let mut needs = vec!["gate".to_owned()];
    let source = source_of(plan);
    let mut conditions = vec![GATE_IS_READY.to_owned(), source.passed.to_owned()];
    let mut steps = vec![checkout(source.verified, 1)];
    if check.stage == CheckStage::BeforePublish {
        if plan.family.is_some() {
            needs.push("family".to_owned());
            conditions.push(BUILD_OUTPUT.to_owned());
        } else {
            needs.push("tag".to_owned());
            let packages: BTreeSet<&str> = check
                .artifacts
                .iter()
                .map(|artifact| artifact.package.as_str())
                .collect();
            for package in packages {
                conditions.push(format!("needs.tag.outputs.{package} != ''"));
            }
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
