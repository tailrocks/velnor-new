// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{JobAuthorityLimit, RepoPath};

use super::{JobAuthorityError, inventory::Context};

pub(super) fn within(
    limit: JobAuthorityLimit,
    at: impl FnOnce() -> String,
    listed: usize,
) -> Result<(), JobAuthorityError> {
    if listed > limit.max() {
        return Err(JobAuthorityError::OverLimit {
            limit,
            at: at(),
            listed,
        });
    }
    Ok(())
}

const REF_PREFIX: &str = "refs/heads/";

pub(super) fn workflow(path: &RepoPath, contexts: &[Context]) -> Result<(), JobAuthorityError> {
    within(
        JobAuthorityLimit::TextBytes,
        || format!("the workflow path {path}"),
        path.as_str().len(),
    )?;
    for context in contexts {
        for branches in [&context.branch_filters, &context.pull_request_base_branches] {
            within(
                JobAuthorityLimit::Refs,
                || format!("{} of {path}", context.event),
                branches.len(),
            )?;
            for branch in branches {
                within(
                    JobAuthorityLimit::TextBytes,
                    || format!("the ref {REF_PREFIX}{branch} of {path}"),
                    REF_PREFIX.len() + branch.len(),
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn job(path: &RepoPath, key: &str, needs: &[String]) -> Result<(), JobAuthorityError> {
    let at = || format!("job {key} of {path}");
    within(JobAuthorityLimit::JobKeyBytes, at, key.len())?;
    within(JobAuthorityLimit::Needs, at, needs.len())?;
    for need in needs {
        within(
            JobAuthorityLimit::JobKeyBytes,
            || format!("job {need}, a need of {key} in {path},"),
            need.len(),
        )?;
    }
    Ok(())
}
