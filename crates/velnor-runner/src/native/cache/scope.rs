// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_cache_rules::{Access, Event, access_for};

use crate::exec::{ExecError, JobFacts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheMode {
    Read,
    Write,
    WriteOnly,
    None,
}

impl CacheMode {
    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "write-only" => Some(Self::WriteOnly),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    pub(super) fn can_read(self) -> bool {
        matches!(self, Self::Read | Self::Write)
    }

    pub(super) fn can_write(self) -> bool {
        matches!(self, Self::Write | Self::WriteOnly)
    }
}

impl fmt::Display for CacheMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::WriteOnly => "write-only",
            Self::None => "none",
        })
    }
}

#[derive(Debug)]
pub(super) struct CacheAccess {
    pub(super) rules: Access,
    pub(super) mode: CacheMode,
}

fn unsupported(job: &JobFacts) -> ExecError {
    ExecError::Unsupported(format!(
        "actions/cache event '{}' with ref '{}' is not supported by velnor-runner",
        job.event_name, job.git_ref,
    ))
}

fn event_of(job: &JobFacts) -> Result<Event, ExecError> {
    let branch = job.git_ref.strip_prefix("refs/heads/");
    if job.event_name == "merge_group" {
        return branch
            .map(|queue_branch| Event::MergeGroup {
                queue_branch: queue_branch.to_owned(),
            })
            .ok_or_else(|| unsupported(job));
    }
    match (
        job.pull_request,
        branch,
        job.git_ref.strip_prefix("refs/tags/"),
    ) {
        (Some(number), _, _) if !job.base_ref.is_empty() => Ok(Event::PullRequest {
            number,
            base_branch: job.base_ref.clone(),
        }),
        (None, Some(branch), _) => Ok(Event::Push {
            branch: branch.to_owned(),
        }),
        (None, None, Some(name)) => Ok(Event::Tag {
            name: name.to_owned(),
        }),
        _ => Err(unsupported(job)),
    }
}

pub(super) fn mode_of(job: &JobFacts) -> Result<CacheMode, ExecError> {
    match job.cache_mode.as_deref() {
        None => Ok(CacheMode::Write),
        Some(text) => CacheMode::parse(text).ok_or_else(|| {
            ExecError::Unsupported(format!(
                "actions/cache mode '{text}' is not supported by velnor-runner"
            ))
        }),
    }
}

pub(super) fn cache_access(job: &JobFacts) -> Result<CacheAccess, ExecError> {
    let default_branch = job.default_branch.as_deref().ok_or_else(|| {
        ExecError::Unsupported(
            "actions/cache needs github.event.repository.default_branch".to_owned(),
        )
    })?;
    Ok(CacheAccess {
        rules: access_for(&event_of(job)?, default_branch),
        mode: mode_of(job)?,
    })
}
