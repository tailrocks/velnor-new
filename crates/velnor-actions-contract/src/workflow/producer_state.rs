// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_access::SOURCES_PRODUCER_JOB_ID;
use super::cache_family::RootSlug;
use super::crate_job::TOOLS_PRODUCER_JOB_ID_PREFIX;
use super::state_seal::{
    STATE_PRODUCER_JOB_ID_PREFIX, STATE_PRODUCER_MAX_PARALLEL, STATE_SEAL_ADMIT_CONDITION,
    STATE_SEAL_ATTEST_CONDITION, STATE_SEAL_BIND_CONDITION, STATE_SEAL_JOB_ID,
    STATE_SEAL_MATRIX_OUTPUT, STATE_SEAL_MAX_PARALLEL, STATE_SEAL_SAVE_CONDITION,
    STATE_SEAL_TIP_CONDITION, state_producer_matrix_output,
};
use super::{Job, JobMatrix, PLAN_JOB_ID, Step, StepKind, StepRole};

const PRODUCER_SEQUENCE: [StepRole; 7] = [
    StepRole::MbxBundleImport,
    StepRole::StateSkeleton,
    StepRole::StateSkeletonRestore,
    StepRole::MbxProducerMark,
    StepRole::MbxBundleExport,
    StepRole::StateSealUpload,
    StepRole::StateProduced,
];

const SEAL_SEQUENCE: [StepRole; 8] = [
    StepRole::StateSealDownload,
    StepRole::StateSealSubject,
    StepRole::StateSealTip,
    StepRole::StateSealAdmit,
    StepRole::StateSealSave,
    StepRole::StateSealBind,
    StepRole::StateSealAttest,
    StepRole::StateSealRetire,
];

fn position(steps: &[Step], role: StepRole) -> Option<usize> {
    let mut found = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(role))
        .map(|(index, _)| index);
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

fn lane_digests(steps: &[Step]) -> Vec<(&str, Option<&str>)> {
    steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::TaskExecution { task_digest, .. } => {
                Some((task_digest.as_str(), step.condition.as_deref()))
            }
            _ => None,
        })
        .collect()
}

fn matrix_ok(job: &Job, output: &str, max_parallel: u32) -> bool {
    matches!(&job.matrix, Some(JobMatrix { include_output, max_parallel: cap, legs })
        if include_output == output && *cap == max_parallel && *legs >= 1)
}

pub(super) fn state_producer_shape_ok(id: &str, job: &Job) -> bool {
    let Some(root) = id
        .strip_prefix(STATE_PRODUCER_JOB_ID_PREFIX)
        .and_then(|root| RootSlug::from_slug(root).ok())
    else {
        return false;
    };
    if job.needs != [PLAN_JOB_ID]
        || !matrix_ok(
            job,
            &state_producer_matrix_output(&root),
            STATE_PRODUCER_MAX_PARALLEL,
        )
    {
        return false;
    }
    let steps = &job.steps;
    let at: Option<Vec<usize>> = PRODUCER_SEQUENCE
        .iter()
        .map(|role| position(steps, *role))
        .collect();
    let Some(at) = at else {
        return false;
    };
    let [_, skeleton, restore, mark, .., produced] = at[..] else {
        return false;
    };
    let tail = &steps[produced + 1..];
    at.windows(2).all(|pair| pair[0] < pair[1])
        && (tail.is_empty()
            || matches!(tail, [step] if step.role == Some(StepRole::MbxWorkspaceCleanup)))
        && lane_digests(&steps[..skeleton]).is_empty()
        && lane_digests(&steps[mark..]).is_empty()
        && lane_digests(&steps[skeleton..restore]) == lane_digests(&steps[restore..mark])
        && !lane_digests(&steps[skeleton..restore]).is_empty()
}

pub(super) fn state_seal_shape_ok(id: &str, job: &Job) -> bool {
    let [plan, rest @ ..] = job.needs.as_slice() else {
        return false;
    };
    id == STATE_SEAL_JOB_ID
        && plan == PLAN_JOB_ID
        && matrix_ok(job, STATE_SEAL_MATRIX_OUTPUT, STATE_SEAL_MAX_PARALLEL)
        && rest
            .iter()
            .any(|need| need.starts_with(STATE_PRODUCER_JOB_ID_PREFIX))
        && rest.iter().all(|need| {
            need == SOURCES_PRODUCER_JOB_ID
                || need.starts_with(TOOLS_PRODUCER_JOB_ID_PREFIX)
                || need.starts_with(STATE_PRODUCER_JOB_ID_PREFIX)
        })
        && job.steps.len() == SEAL_SEQUENCE.len()
        && job
            .steps
            .iter()
            .zip(SEAL_SEQUENCE)
            .all(|(step, role)| step.role == Some(role))
        && job.steps.iter().all(|step| {
            let expected = match step.role {
                Some(StepRole::StateSealSave) => Some(STATE_SEAL_SAVE_CONDITION),
                Some(StepRole::StateSealBind) => Some(STATE_SEAL_BIND_CONDITION),
                Some(StepRole::StateSealTip) => Some(STATE_SEAL_TIP_CONDITION),
                Some(StepRole::StateSealAdmit) => Some(STATE_SEAL_ADMIT_CONDITION),
                Some(StepRole::StateSealAttest | StepRole::StateSealRetire) => {
                    Some(STATE_SEAL_ATTEST_CONDITION)
                }
                _ => return true,
            };
            step.condition.as_deref() == expected
        })
}
