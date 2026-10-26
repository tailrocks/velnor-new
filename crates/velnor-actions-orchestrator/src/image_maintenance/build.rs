// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "build_candidate.rs"]
mod candidate;
#[path = "build_install.rs"]
mod install;
#[path = "build_support.rs"]
pub(super) mod support;

use crate::OrchestratorError;

use super::{BuildJobCandidateRequest, InstallBuildxRequest};

pub(super) fn install_buildx(request: &InstallBuildxRequest) -> Result<(), OrchestratorError> {
    install::install_buildx(request)
}

pub(super) fn build_job_candidate(
    request: &BuildJobCandidateRequest,
) -> Result<(), OrchestratorError> {
    candidate::build_job_candidate(request)
}
