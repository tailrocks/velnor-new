// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::entry::Scope;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Push { branch: String },
    Tag { name: String },
    PullRequest { number: u64, base_branch: String },
    MergeGroup { queue_branch: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    pub read: Vec<Scope>,
    pub write: Scope,
}

#[must_use]
pub fn access_for(event: &Event, default_branch: &str) -> Access {
    let (write, base) = match event {
        Event::Push { branch } => (Scope::branch(branch), None),
        Event::Tag { name } => (Scope::tag(name), None),
        Event::PullRequest {
            number,
            base_branch,
        } => (Scope::pull_merge(*number), Some(Scope::branch(base_branch))),
        Event::MergeGroup { queue_branch } => (Scope::branch(queue_branch), None),
    };
    let mut read = vec![write.clone()];
    for scope in base.into_iter().chain([Scope::branch(default_branch)]) {
        if !read.contains(&scope) {
            read.push(scope);
        }
    }
    Access { read, write }
}
