// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperCommand, Job, Step, StepKind, When};

use crate::plan::step;

pub(crate) fn upload(jobs: &mut [Job]) {
    for job in jobs {
        if job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Cargo(_)))
        {
            let gate = job
                .steps
                .iter()
                .find(|step| {
                    matches!(
                        step.kind,
                        StepKind::Helper(HelperCommand::Restored(..))
                            | StepKind::RestoredTargets { .. }
                    )
                })
                .and_then(|step| step.gate.clone());
            job.steps.push(Step {
                when: When::CacheDebug,
                gate,
                ..step(StepKind::UploadCargoLog)
            });
        }
    }
}
