// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperCommand, Job, PrCi, RepoFacts, StepKind};

use crate::lane::PLAN_JOB;
use crate::plan::{RUST_JOB, step};

const QUEUE_CHECK_HELPER: (u64, u64, u64) = (0, 1, 100);

/// Label mode lets unlabeled code merge unless a merge queue runs the full
/// suite, and the generator cannot see repository settings. Right after the
/// plan, the helper reads the rulesets of the base branch and warns.
pub(crate) fn ask(facts: &RepoFacts, jobs: &mut [Job], single: bool) {
    let known = facts
        .helper
        .as_ref()
        .is_some_and(|release| release.version().at_least(QUEUE_CHECK_HELPER));
    if facts.config.pr_ci != PrCi::Label || !known {
        return;
    }
    let asker = if single { RUST_JOB } else { PLAN_JOB };
    for job in jobs.iter_mut().filter(|job| job.id == asker) {
        if let Some(at) = job
            .steps
            .iter()
            .position(|step| step.kind == StepKind::Helper(HelperCommand::Plan))
        {
            job.steps.insert(at + 1, step(StepKind::MergeQueueCheck));
        }
    }
}
