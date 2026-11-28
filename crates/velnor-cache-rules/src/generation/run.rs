// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::access::{Event, access_for};
use crate::entry::Scope;
use crate::generation::key::Sha;
use crate::generation::scope::GenScope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    SameRepository,
    Fork,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunEvent {
    Push {
        branch: String,
    },
    Tag {
        name: String,
    },
    PullRequest {
        number: u64,
        base_branch: String,
        head: Head,
    },
    MergeGroup {
        queue_branch: String,
    },
}

impl RunEvent {
    fn access_event(&self) -> Event {
        match self {
            Self::Push { branch } => Event::Push {
                branch: branch.clone(),
            },
            Self::Tag { name } => Event::Tag { name: name.clone() },
            Self::PullRequest {
                number,
                base_branch,
                ..
            } => Event::PullRequest {
                number: *number,
                base_branch: base_branch.clone(),
            },
            Self::MergeGroup { queue_branch } => Event::MergeGroup {
                queue_branch: queue_branch.clone(),
            },
        }
    }

    fn is_fork(&self) -> bool {
        matches!(
            self,
            Self::PullRequest {
                head: Head::Fork,
                ..
            }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub event: RunEvent,
    pub default_branch: String,
    pub sha: Sha,
    pub run_id: u64,
    pub attempt: u32,
    pub protected_ref: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Pr(u64),
    Mg(Sha),
    Main,
}

impl Run {
    pub(crate) fn lanes(&self) -> Vec<Lane> {
        if self.event.is_fork() {
            return Vec::new();
        }
        let access = access_for(&self.event.access_event(), &self.default_branch);
        let default_scope = Scope::branch(&self.default_branch);
        let mut lanes = Vec::new();
        if access.write == default_scope {
            lanes.push(Lane::Mg(self.sha.clone()));
        }
        for scope in &access.read {
            match &self.event {
                RunEvent::PullRequest { number, .. } if *scope == Scope::pull_merge(*number) => {
                    lanes.push(Lane::Pr(*number));
                }
                _ if *scope == default_scope => lanes.push(Lane::Main),
                _ => {}
            }
        }
        lanes
    }

    #[must_use]
    pub fn publishes(&self) -> Option<GenScope> {
        match &self.event {
            RunEvent::Push { branch } if self.protected_ref && *branch == self.default_branch => {
                Some(GenScope::Main {
                    sha: self.sha.clone(),
                })
            }
            RunEvent::MergeGroup { .. } => Some(GenScope::Mg {
                sha: self.sha.clone(),
            }),
            RunEvent::PullRequest {
                number,
                head: Head::SameRepository,
                ..
            } => Some(GenScope::Pr {
                number: *number,
                run_id: self.run_id,
                attempt: self.attempt,
            }),
            _ => None,
        }
    }
}
