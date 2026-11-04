// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_access::SOURCES_PRODUCER_JOB_ID;
use super::crate_job::{TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX, TOOLS_PRODUCER_JOB_ID_PREFIX};
use super::state_seal::{
    STATE_PRODUCER_JOB_ID_PREFIX, STATE_SEAL_ATTEST_CONDITION, STATE_SEAL_BIND_CONDITION,
    STATE_SEAL_JOB_ID_PREFIX, STATE_SEAL_SAVE_CONDITION,
};
use super::{Job, PLAN_JOB_ID, Step, StepKind, StepRole};

const PRODUCER_SEQUENCE: [StepRole; 7] = [
    StepRole::MbxBundleImport,
    StepRole::StateSkeleton,
    StepRole::StateSkeletonRestore,
    StepRole::MbxProducerMark,
    StepRole::MbxBundleExport,
    StepRole::StateSealUpload,
    StepRole::StateProduced,
];

const SEAL_SEQUENCE: [StepRole; 7] = [
    StepRole::StateSealDownload,
    StepRole::StateSealSubject,
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

fn lane_digests(steps: &[Step]) -> Vec<&str> {
    steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::TaskExecution { task_digest, .. } => Some(task_digest.as_str()),
            _ => None,
        })
        .collect()
}

pub(super) fn state_producer_shape_ok(job: &Job) -> bool {
    if job.needs != [PLAN_JOB_ID] {
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
    let Some(root) = id.strip_prefix(STATE_SEAL_JOB_ID_PREFIX) else {
        return false;
    };
    let producer = format!("{STATE_PRODUCER_JOB_ID_PREFIX}{root}");
    let [plan, built, rest @ ..] = job.needs.as_slice() else {
        return false;
    };
    plan == PLAN_JOB_ID
        && *built == producer
        && rest.iter().all(|need| {
            need == SOURCES_PRODUCER_JOB_ID
                || need.starts_with(TOOLS_PRODUCER_JOB_ID_PREFIX)
                || need.starts_with(TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX)
                || (need.starts_with(STATE_SEAL_JOB_ID_PREFIX) && need != id)
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
                Some(StepRole::StateSealAttest | StepRole::StateSealRetire) => {
                    Some(STATE_SEAL_ATTEST_CONDITION)
                }
                _ => return true,
            };
            step.condition.as_deref() == expected
        })
}
