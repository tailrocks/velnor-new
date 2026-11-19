// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{CacheKey, CacheRoot, JobId, LockIdentity, RepoPath, RootSlug};

pub const CACHE_COMPAT_LENGTH: usize = 16;
pub const LOCK_OUTPUT_PREFIX: &str = "lock_";
pub const RUNNER_OS_MAX_LENGTH: usize = "Windows".len();
pub const RUNNER_ARCH_MAX_LENGTH: usize = "ARM64".len();
pub const SHARED_SOURCES_OWNER: &str = "all.roots";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheKeySegment {
    Text(String),
    Host,
    TargetOwner { job: JobId, root: RootSlug },
    Compat,
    Locks(Vec<RepoPath>),
    RootLock(RootSlug),
    SourcesLocks,
    Mise,
    Commit,
    RunId,
    DeltaBase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKeyLayout {
    pub segments: Vec<CacheKeySegment>,
    pub fallback_lengths: &'static [usize],
}

impl CacheKeySegment {
    #[must_use]
    pub fn json_content_max_bytes(&self) -> usize {
        match self {
            Self::Text(text) => json_string_bytes(text).saturating_sub(2),
            Self::Host => RUNNER_OS_MAX_LENGTH + 1 + RUNNER_ARCH_MAX_LENGTH,
            Self::TargetOwner { job, root } => json_string_bytes(job.as_str())
                .saturating_sub(2)
                .saturating_add(1)
                .saturating_add(json_string_bytes(root.as_str()).saturating_sub(2))
                .saturating_add(1 + (RUNNER_OS_MAX_LENGTH + 1 + RUNNER_ARCH_MAX_LENGTH)),
            Self::Compat => CACHE_COMPAT_LENGTH,
            _ => usize::MAX,
        }
    }
}

impl CacheKey {
    #[must_use]
    pub fn layout(&self) -> CacheKeyLayout {
        use CacheKeySegment::{
            Commit, Compat, DeltaBase, Host, Locks, Mise, RunId, SourcesLocks, TargetOwner, Text,
        };
        let (segments, fallback_lengths): (Vec<_>, &'static [usize]) = match self {
            Self::Tools { .. } => (
                vec![
                    Text("velnor".into()),
                    Text("tools".into()),
                    Host,
                    Mise,
                    Locks(vec![RepoPath::from_static("mise.lock")]),
                ],
                &[],
            ),
            Self::Sources { .. } => (
                vec![
                    Text("velnor".into()),
                    Text("src".into()),
                    Host,
                    Text(SHARED_SOURCES_OWNER.into()),
                    SourcesLocks,
                ],
                &[4],
            ),
            Self::Target { job, root, .. } => (
                vec![
                    Text("velnor".into()),
                    Text("tgt".into()),
                    TargetOwner {
                        job: job.clone(),
                        root: root.slug().clone(),
                    },
                    Compat,
                    lock_segment(root),
                    Commit,
                    RunId,
                ],
                &[5, 4],
            ),
            Self::Delta { job, root, .. } => (
                vec![
                    Text("velnor".into()),
                    Text("dlt".into()),
                    Text(job.to_string()),
                    Text(root.slug().to_string()),
                    Host,
                    Compat,
                    DeltaBase,
                    RunId,
                ],
                &[7],
            ),
        };
        CacheKeyLayout {
            segments,
            fallback_lengths,
        }
    }
}

fn lock_segment(root: &CacheRoot) -> CacheKeySegment {
    match root.lock_identity() {
        LockIdentity::File => CacheKeySegment::Locks(root.locks().to_vec()),
        LockIdentity::Members => CacheKeySegment::RootLock(root.slug().clone()),
    }
}

#[must_use]
pub fn json_string_bytes(text: &str) -> usize {
    text.chars()
        .fold(2_usize, |bytes, c| bytes.saturating_add(json_char_bytes(c)))
}

#[must_use]
pub const fn json_char_bytes(c: char) -> usize {
    match c {
        '"' | '\\' => 2,
        c if c.is_control() => 6,
        c if c.is_ascii() => 1,
        c => 6 * c.len_utf16(),
    }
}
