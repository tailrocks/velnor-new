// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Platform;
use velnor_release::{BinaryPlan, ReleasePlan};

use crate::release::GATE_IS_READY;
use crate::release_assets::published_files;
use crate::release_binaries::{DIST, build_id, download};
use crate::release_family::{BUILD_OUTPUT, after_the_family};
use crate::release_source::Source;
use crate::release_steps::expression;
use crate::rules::{RenderedJob, RenderedStep};

pub(crate) fn is_optional(binary: &BinaryPlan, platform: Platform) -> bool {
    binary.optional_platforms.contains(&platform)
}

pub(crate) fn optional_files(binary: &BinaryPlan) -> Vec<String> {
    binary
        .optional_platforms
        .iter()
        .flat_map(|platform| published_files(binary, *platform))
        .collect()
}

pub(crate) fn exists(name: &str) -> String {
    format!("hashFiles('{DIST}/{name}') != ''")
}

pub(crate) fn when_present(name: &str, also: Option<&str>) -> String {
    let present = exists(name);
    expression(&also.map_or(present.clone(), |first| format!("{first} && {present}")))
}

pub(crate) fn download_optional(name: String) -> RenderedStep {
    RenderedStep {
        continue_on_error: true,
        ..download(name)
    }
}

pub(crate) fn artifact_download(name: String, optional: bool) -> RenderedStep {
    if optional {
        download_optional(name)
    } else {
        download(name)
    }
}

pub(crate) fn has_optional(plan: &ReleasePlan) -> bool {
    plan.binaries
        .iter()
        .any(|binary| !binary.optional_platforms.is_empty())
}

pub(crate) fn after_the_builds(
    id: &str,
    plan: &ReleasePlan,
    builds: Vec<String>,
    source: &Source,
) -> RenderedJob {
    let job = after_the_family(id, builds, source);
    if !has_optional(plan) {
        return job;
    }
    let required = ["gate".to_owned(), "family".to_owned()]
        .into_iter()
        .chain(plan.binaries.iter().flat_map(|binary| {
            binary
                .platforms
                .iter()
                .filter(|platform| !is_optional(binary, **platform))
                .map(|platform| build_id(&binary.package, *platform))
        }))
        .map(|need| format!("needs.{need}.result == 'success'"))
        .collect::<Vec<_>>()
        .join(" && ");
    RenderedJob {
        condition: Some(expression(&format!(
            "always() && {required} && {GATE_IS_READY} && {} && {BUILD_OUTPUT}",
            source.passed
        ))),
        ..job
    }
}
