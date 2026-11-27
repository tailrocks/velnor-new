// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestHead {
    SameRepository,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrustedEvent {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrustedFacts {
    pub repository_id: u64,
    pub repository: String,
    pub server_url: String,
    pub default_branch: String,
    pub sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub event_name: String,
    pub event: TrustedEvent,
}

impl TrustedFacts {
    #[must_use]
    pub fn to_file(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}
