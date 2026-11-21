// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Job, StepKind};

use crate::rules::RenderedStep;

const MIN_RUN_BYTES: usize = 100;

pub(crate) fn mark(job: &Job, steps: &mut [RenderedStep]) {
    let Some(release) = job.steps.iter().find_map(|step| match &step.kind {
        StepKind::InstallHelper(release) => Some(release),
        _ => None,
    }) else {
        return;
    };
    if !release.version().at_least((0, 1, 111)) {
        return;
    }
    for step in steps {
        step.share_run = step
            .run
            .as_ref()
            .is_some_and(|run| run.len() >= MIN_RUN_BYTES && !run.contains('\n'));
    }
}
