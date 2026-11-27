// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use ring::digest::{SHA256, digest};

use super::facts::{TrustedEvent, TrustedFacts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrustedRef {
    remote: String,
    local: String,
}

impl TrustedRef {
    pub(crate) fn of(facts: &TrustedFacts) -> Result<Self, String> {
        let remote = match (&facts.event, facts.event_name.as_str()) {
            (TrustedEvent::Tag { name }, _) => format!("refs/tags/{name}"),
            (TrustedEvent::Branch { branch }, "push") => format!("refs/heads/{branch}"),
            _ if facts.default_branch.is_empty() => {
                return Err("the job has no trusted ref: its default branch is unknown".to_owned());
            }
            _ => format!("refs/heads/{}", facts.default_branch),
        };
        let digest = digest(&SHA256, remote.as_bytes()).as_ref().iter().fold(
            String::new(),
            |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            },
        );
        let local = format!("refs/velnor/trusted/{digest}");
        Ok(Self { remote, local })
    }

    pub(crate) fn remote(&self) -> &str {
        &self.remote
    }

    pub(crate) fn local(&self) -> &str {
        &self.local
    }

    pub(super) fn refspec(&self) -> String {
        format!("+{}:{}", self.remote, self.local)
    }
}
