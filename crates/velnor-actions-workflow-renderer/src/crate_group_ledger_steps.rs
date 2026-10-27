// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::crate_job::{GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX};
use velnor_actions_contract::{Job, Step, StepKind, WorkflowPolicy};

use crate::steps_internal::group_ledger_steps::{
    EMIT_GROUP_LEDGER_OPERATION, EMIT_GROUP_LEDGER_STEP_NAME, MATRIX_KEY_ENV,
    emit_group_ledger_step,
};

pub(crate) fn append(jobs: &mut BTreeMap<String, Job>, policy: WorkflowPolicy) {
    for (id, job) in jobs {
        if !id.ends_with(GROUP_READER_SUFFIX) && !id.ends_with(GROUP_WRITER_SUFFIX) {
            continue;
        }
        let upload = job.steps.iter().enumerate().find_map(|(index, step)| {
            let StepKind::Action { uses, with, .. } = &step.kind else {
                return None;
            };
            if uses != crate::steps::UPLOAD_ARTIFACT_USES {
                return None;
            }
            let name = with.get("name")?;
            if name.starts_with("velnor-matrix-") {
                Some((index, true))
            } else if name.starts_with("velnor-crate-") {
                Some((index, false))
            } else {
                None
            }
        });
        let Some((upload, matrix)) = upload else {
            continue;
        };
        let step = emitter(policy, matrix);
        if let Some(at) = job
            .steps
            .iter()
            .position(|step| step.name == EMIT_GROUP_LEDGER_STEP_NAME)
        {
            job.steps[at] = step;
        } else {
            job.steps.insert(upload, step);
        }
    }
}

fn emitter(policy: WorkflowPolicy, matrix: bool) -> Step {
    let mut step = emit_group_ledger_step();
    if let StepKind::Shell { env, .. } = &mut step.kind {
        if !matrix {
            env.remove(MATRIX_KEY_ENV);
        }
        if policy == WorkflowPolicy::ConsumerV1 {
            let mut env = std::mem::take(env);
            env.remove(crate::steps::INTERNAL_OP_ENV);
            step.kind = StepKind::Internal {
                operation: EMIT_GROUP_LEDGER_OPERATION.to_owned(),
                env,
            };
        }
    }
    step
}
