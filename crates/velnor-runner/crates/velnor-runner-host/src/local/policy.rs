// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::config::{
    ApprovedJobAuthorityPolicy, JobAuthorityLookup, PullRequestRule, WorkflowEvent,
};
use crate::error::HostError;
use crate::launch::execution_authorization::ExecutionSourceRequest;

use super::plan::Side;

const MAX_POLICY_BYTES: u64 = 256 * 1024;

#[derive(Debug)]
pub(super) struct Policy(ApprovedJobAuthorityPolicy);

impl Policy {
    pub(super) fn load(state: &Path, name: &str) -> Result<Self, HostError> {
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(HostError::Config);
        }
        let path = state.join(name);
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| HostError::Config)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_POLICY_BYTES {
            return Err(HostError::Config);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|_| HostError::Config)?
            .take(MAX_POLICY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| HostError::Config)?;
        Self::parse(&bytes)
    }

    pub(super) fn parse(bytes: &[u8]) -> Result<Self, HostError> {
        let mut policy: ApprovedJobAuthorityPolicy =
            serde_json::from_slice(bytes).map_err(|_| HostError::Config)?;
        policy.canonicalize();
        Ok(Self(policy))
    }

    pub(super) fn approves(
        &self,
        request: &ExecutionSourceRequest,
        side: Side,
        workflow_sha256: Option<&str>,
    ) -> bool {
        if side == Side::Untrusted {
            return false;
        }
        let Ok(repository_id) = i64::try_from(request.repository_id()) else {
            return false;
        };
        let Some(root) =
            super::decide::workflow_path(request.workflow_ref(), request.repository_name())
        else {
            return false;
        };
        let Some(content) = workflow_sha256 else {
            return false;
        };
        let standalone = self.0.workflow_sources().any(|source| {
            source.repository_id() == repository_id
                && source.path() == root
                && source.content_sha256() == content
                && source.calls().next().is_none()
        });
        let Ok(event) =
            serde_json::from_value::<WorkflowEvent>(Value::String(request.event_name().to_owned()))
        else {
            return false;
        };
        let pull_request = request.pull_request_number().map(|number| PullRequestRule {
            number: Some(number),
            base_repository_id: repository_id,
            base_branch: request.base_ref().unwrap_or_default().to_owned(),
            head_repository_id: repository_id,
            head_branch: request.head_ref().unwrap_or_default().to_owned(),
        });
        let lookup = JobAuthorityLookup {
            root_path: root,
            workflow_path: root,
            job_key: request.job_key(),
            event,
            event_ref: if event == WorkflowEvent::PullRequest {
                ""
            } else {
                request.event_ref()
            },
            pull_request: pull_request.as_ref(),
        };
        standalone && self.0.entry(&lookup).is_some()
    }
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
