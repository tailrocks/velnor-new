// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Job, StepKind};

pub(crate) fn release_of(job: &Job) -> Option<&HelperRelease> {
    job.steps.iter().find_map(|step| match &step.kind {
        StepKind::InstallHelper(release) => Some(release),
        _ => None,
    })
}

pub(crate) fn at_least(job: &Job, version: (u64, u64, u64)) -> bool {
    release_of(job).is_some_and(|release| release.version().at_least(version))
}
