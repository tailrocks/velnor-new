// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, RepoView, StepKind, WorkflowPlan};

use crate::GenerateError;

pub(crate) fn validate_staging(view: &RepoView, plan: &WorkflowPlan) -> Result<(), GenerateError> {
    for root in plan
        .jobs
        .iter()
        .flat_map(|job| &job.steps)
        .filter_map(|step| {
            if let StepKind::CacheSave(CacheKey::Delta { root, .. }) = &step.kind {
                Some(root)
            } else {
                None
            }
        })
    {
        let staging = root.delta_dir();
        if let Some(path) = view.tracked_paths.iter().find(|path| {
            path.as_str() == staging || path.as_str().starts_with(&format!("{staging}/"))
        }) {
            return Err(GenerateError::DeltaStagingCollision(path.clone()));
        }
    }
    Ok(())
}
