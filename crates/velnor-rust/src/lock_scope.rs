// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{CacheKey, Job, LockIdentity, PrepareLocks, RepoFacts, RootSlug, StepKind};

const SCOPED_LOCKS_HELPER: (u64, u64, u64) = (0, 1, 104);

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    let scoped = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(SCOPED_LOCKS_HELPER));
    if !scoped {
        return;
    }
    for job in jobs {
        let roots: BTreeSet<RootSlug> = job
            .steps
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::CacheRestore(CacheKey::Target { root, .. })
                | StepKind::CacheSave(CacheKey::Target { root, .. })
                    if root.lock_identity() == LockIdentity::Members =>
                {
                    Some(root.slug().clone())
                }
                _ => None,
            })
            .collect();
        for step in &mut job.steps {
            if let StepKind::Prepare(locks) = &mut step.kind {
                *locks = PrepareLocks::Only(roots.iter().cloned().collect());
            }
        }
    }
}
