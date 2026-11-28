// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PullRequestHead {
    SameRepository,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TrustedEvent {
    Branch {
        branch: String,
    },
    Tag {
        name: String,
    },
    PullRequest {
        number: u64,
        base_branch: String,
        head: PullRequestHead,
    },
    MergeGroup {
        queue_branch: String,
    },
}

pub(crate) struct Verified {
    pub(crate) repository_id: u64,
    pub(crate) repository: String,
    pub(crate) server_url: String,
    pub(crate) default_branch: String,
    pub(crate) sha: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) event_name: String,
    pub(crate) event: TrustedEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrustedFacts {
    repository_id: u64,
    repository: String,
    server_url: String,
    default_branch: String,
    sha: String,
    run_id: u64,
    run_attempt: u32,
    event_name: String,
    event: TrustedEvent,
}

impl TrustedFacts {
    pub(crate) fn new(verified: Verified) -> Self {
        Self {
            repository_id: verified.repository_id,
            repository: verified.repository,
            server_url: verified.server_url,
            default_branch: verified.default_branch,
            sha: verified.sha,
            run_id: verified.run_id,
            run_attempt: verified.run_attempt,
            event_name: verified.event_name,
            event: verified.event,
        }
    }

    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn for_test() -> Self {
        Self::new(Verified {
            repository_id: 42,
            repository: "tailrocks/velnor-lab".to_owned(),
            server_url: "https://github.com".to_owned(),
            default_branch: "main".to_owned(),
            sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            run_id: 1001,
            run_attempt: 1,
            event_name: "pull_request".to_owned(),
            event: TrustedEvent::PullRequest {
                number: 7,
                base_branch: "main".to_owned(),
                head: PullRequestHead::SameRepository,
            },
        })
    }

    #[must_use]
    pub(crate) fn to_file(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}
