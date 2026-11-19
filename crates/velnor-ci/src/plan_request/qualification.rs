// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::write_request::internal;

const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Cold,
    Warm,
    Third,
    UsefulDelta,
    Control,
}

impl Phase {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::Third => "third",
            Self::UsefulDelta => "useful_delta",
            Self::Control => "control",
        }
    }

    pub(super) const fn cache_enabled(self) -> bool {
        !matches!(self, Self::Control)
    }

    const fn needs_predecessor(self) -> bool {
        matches!(self, Self::Warm | Self::Third | Self::UsefulDelta)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRef {
    run_id: u64,
    run_attempt: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Dispatch {
    pub(super) campaign: String,
    pub(super) phase: Phase,
    repository: String,
    default_branch: String,
    git_ref: String,
    ref_protected: bool,
    workflow_ref: String,
    workflow_sha: String,
    source_sha: String,
    run_id: u64,
    run_attempt: u32,
    #[serde(default)]
    predecessor: Option<RunRef>,
}

fn invalid(field: &str, problem: &str) -> CiError {
    internal(&format!("invalid {field}: {problem}"))
}

impl Dispatch {
    pub(super) fn validate_for(
        &self,
        default_branch: &str,
        expected_repository: &str,
        expected_source_sha: &str,
    ) -> Result<(), CiError> {
        self.validate_shape()?;
        if self.repository != expected_repository {
            return Err(invalid("qualification.repository", "repository_mismatch"));
        }
        if self.default_branch != default_branch
            || self.git_ref != format!("refs/heads/{default_branch}")
        {
            return Err(invalid("qualification.ref", "not_default_branch"));
        }
        if !self.ref_protected {
            return Err(invalid("qualification.ref_protected", "unprotected_ref"));
        }
        let expected = format!(
            "{}/{CI_WORKFLOW_PATH}@refs/heads/{default_branch}",
            self.repository
        );
        if self.workflow_ref != expected {
            return Err(invalid(
                "qualification.workflow_ref",
                "workflow_ref_mismatch",
            ));
        }
        if self.source_sha != expected_source_sha
            || self.workflow_sha != expected_source_sha
            || self.source_sha != self.workflow_sha
        {
            return Err(invalid(
                "qualification.source_sha",
                "source_or_workflow_sha_mismatch",
            ));
        }
        Ok(())
    }

    pub(super) fn bind_current_sha(&self, current: Option<&str>) -> Result<(), CiError> {
        if current == Some(self.source_sha.as_str()) {
            Ok(())
        } else {
            Err(invalid(
                "qualification.binding",
                "source_sha_not_current_run",
            ))
        }
    }

    pub(super) fn bind_run(&self, run_key: &str) -> Result<(), CiError> {
        if run_key == format!("r{}-a{}", self.run_id, self.run_attempt) {
            Ok(())
        } else {
            Err(invalid("qualification.binding", "source_or_run_mismatch"))
        }
    }

    pub(super) fn require_no_predecessor(&self) -> Result<(), CiError> {
        if self.phase.needs_predecessor() {
            return Err(internal(&format!(
                "qualification_phase_not_supported:{}: the cache directive of a phase with a predecessor is not computed by the helper",
                self.phase.as_str()
            )));
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), CiError> {
        if !valid_campaign(&self.campaign) {
            return Err(invalid("qualification.campaign", "invalid_campaign"));
        }
        if !valid_repository(&self.repository) {
            return Err(invalid("qualification.repository", "invalid_repository"));
        }
        if !lower_hex_sha(&self.source_sha) || !lower_hex_sha(&self.workflow_sha) {
            return Err(invalid("qualification.sha", "invalid_sha"));
        }
        if self.source_sha != self.workflow_sha || !self.ref_protected {
            return Err(invalid(
                "qualification.provenance",
                "unprotected_or_unbound_workflow",
            ));
        }
        self.validate_refs()?;
        if self.run_id == 0 || self.run_attempt == 0 {
            return Err(invalid("qualification.run", "invalid_run_identity"));
        }
        match (self.phase.needs_predecessor(), self.predecessor) {
            (false, Some(_)) => Err(invalid(
                "qualification.predecessor",
                "unexpected_run_reference",
            )),
            (true, None) => Err(invalid(
                "qualification.predecessor",
                "missing_run_reference",
            )),
            (true, Some(reference)) if reference.run_id == 0 || reference.run_attempt == 0 => Err(
                invalid("qualification.predecessor", "invalid_run_reference"),
            ),
            _ => Ok(()),
        }
    }

    fn validate_refs(&self) -> Result<(), CiError> {
        if !valid_branch(&self.default_branch)
            || self.git_ref != format!("refs/heads/{}", self.default_branch)
        {
            return Err(invalid(
                "qualification.default_branch",
                "default_branch_mismatch",
            ));
        }
        let expected = format!("{}/{CI_WORKFLOW_PATH}@{}", self.repository, self.git_ref);
        if self.workflow_ref != expected {
            return Err(invalid(
                "qualification.workflow_ref",
                "workflow_ref_mismatch",
            ));
        }
        Ok(())
    }
}

fn valid_campaign(campaign: &str) -> bool {
    !campaign.is_empty()
        && campaign.len() <= 64
        && campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && campaign
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn valid_repository(repository: &str) -> bool {
    let parts: Vec<&str> = repository.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.starts_with('.')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

fn lower_hex_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
