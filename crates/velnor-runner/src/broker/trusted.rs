// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::facts::{TrustedEvent, TrustedFacts};

const LOCAL: &str = "refs/velnor/trusted/pin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrustedRef {
    remote: String,
}

impl TrustedRef {
    pub(super) fn of(facts: &TrustedFacts) -> Result<Self, String> {
        let remote = match (&facts.event, facts.event_name.as_str()) {
            (TrustedEvent::Tag { name }, _) => format!("refs/tags/{name}"),
            (TrustedEvent::Branch { branch }, "push") => format!("refs/heads/{branch}"),
            _ if facts.default_branch.is_empty() => {
                return Err("the job has no trusted ref: its default branch is unknown".to_owned());
            }
            _ => format!("refs/heads/{}", facts.default_branch),
        };
        Ok(Self { remote })
    }

    pub(super) fn remote(&self) -> &str {
        &self.remote
    }

    pub(super) fn local() -> &'static str {
        LOCAL
    }

    pub(super) fn refspec(&self) -> String {
        format!("+{}:{LOCAL}", self.remote)
    }
}
