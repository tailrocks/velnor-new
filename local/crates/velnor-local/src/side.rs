// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;

use crate::event::{Push, Repository};
use crate::scenario::Step;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Trusted,
    Untrusted,
}

impl Side {
    #[must_use]
    pub fn of(payload: &Value) -> Self {
        if payload["pull_request"]["head"]["repo"]["fork"] == true {
            Self::Untrusted
        } else {
            Self::Trusted
        }
    }

    #[must_use]
    pub fn of_role(role: &str) -> Self {
        if role.starts_with("fork-") {
            Self::Untrusted
        } else {
            Self::Trusted
        }
    }

    pub fn pick<T>(self, trusted: T, untrusted: Option<T>) -> Result<T, String> {
        match (self, untrusted) {
            (Self::Trusted, _) => Ok(trusted),
            (Self::Untrusted, Some(untrusted)) => Ok(untrusted),
            (Self::Untrusted, None) => {
                Err("an untrusted event needs the untrusted side, which is not up".to_owned())
            }
        }
    }
}

#[must_use]
pub fn push_event(
    repository: &Repository<'_>,
    push: &Push<'_>,
    fork_heads: &BTreeSet<String>,
) -> Option<Value> {
    (!fork_heads.contains(push.branch)).then(|| repository.push(push))
}

#[must_use]
pub fn fork_heads(steps: &[Step]) -> BTreeSet<String> {
    steps
        .iter()
        .filter_map(|step| match step {
            Step::OpenPullRequest {
                head, fork: true, ..
            } => Some(head.clone()),
            _ => None,
        })
        .collect()
}

pub fn refuse_fork_branches(steps: &[Step], default_branch: &str) -> Result<(), String> {
    let forks = fork_heads(steps);
    let refuse = |branch: &str, why: &str| {
        if forks.contains(branch) {
            Err(format!(
                "{branch} is the head of a pull request from a fork, so it {why}"
            ))
        } else {
            Ok(())
        }
    };
    refuse(
        default_branch,
        "cannot be the default branch of the repository",
    )?;
    for step in steps {
        match step {
            Step::WorkflowDispatch { branch, .. } => refuse(
                branch,
                "cannot be dispatched: its code would run on the trusted side",
            )?,
            Step::OpenPullRequest { head, base, fork } => {
                refuse(
                    base,
                    "cannot be the base of a pull request: GitHub takes bases from the repository",
                )?;
                if !fork {
                    refuse(
                        head,
                        "cannot also be the head of a pull request from the repository",
                    )?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
