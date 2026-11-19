// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::github::Repo;

pub(super) fn run(run_id: u64) -> Result<String, CiError> {
    let repo =
        std::env::var("GITHUB_REPOSITORY").map_err(|_| CiError::new("set GITHUB_REPOSITORY"))?;
    let github = super::http::HttpGitHub::new(
        super::api_url(),
        super::token()?,
        super::http::DEFAULT_TIMEOUT,
    );
    crate::delta_trim::trim_deltas(&github, &Repo::parse(&repo)?, run_id)
}
