// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::access::{Event, access_for};
use crate::entry::Scope;
use crate::generation::key::Sha;
use crate::generation::scope::GenScope;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub event: Event,
    pub default_branch: String,
    pub sha: Sha,
    pub run_id: u64,
    pub attempt: u32,
    pub same_repository: bool,
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
        if !self.same_repository {
            return Vec::new();
        }
        let access = access_for(&self.event, &self.default_branch);
        let default_scope = Scope::branch(&self.default_branch);
        let mut lanes = Vec::new();
        if access.write == default_scope {
            lanes.push(Lane::Mg(self.sha.clone()));
        }
        for scope in &access.read {
            match &self.event {
                Event::PullRequest { number, .. } if *scope == Scope::pull_merge(*number) => {
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
        let access = access_for(&self.event, &self.default_branch);
        match &self.event {
            Event::Push { .. }
                if self.protected_ref && access.write == Scope::branch(&self.default_branch) =>
            {
                Some(GenScope::Main {
                    sha: self.sha.clone(),
                })
            }
            Event::MergeGroup { .. } => Some(GenScope::Mg {
                sha: self.sha.clone(),
            }),
            Event::PullRequest { number, .. } if self.same_repository => Some(GenScope::Pr {
                number: *number,
                run_id: self.run_id,
                attempt: self.attempt,
            }),
            _ => None,
        }
    }
}
