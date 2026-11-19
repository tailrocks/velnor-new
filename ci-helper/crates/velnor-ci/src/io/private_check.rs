// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{OUTPUT_VARIABLE, append_to};
use crate::error::CiError;
use crate::private_token::check;

const HEAD_REF_VARIABLE: &str = "VELNOR_MERGE_GROUP_HEAD_REF";
const REPOSITORY_VARIABLE: &str = "GITHUB_REPOSITORY";
const SHA_VARIABLE: &str = "GITHUB_SHA";
const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(30);

fn variable(name: &str) -> Result<String, CiError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CiError::new(format!("{name} is not set")))
}

fn decide() -> Result<(), CiError> {
    let head_ref = variable(HEAD_REF_VARIABLE)?;
    let repository = variable(REPOSITORY_VARIABLE)?;
    let event_sha = variable(SHA_VARIABLE)?;
    let github = super::fetch::api_client(variable(TOKEN_VARIABLE)?, LOOKUP_TIMEOUT)?;
    check(&head_ref, &repository, &event_sha, &github)
}

pub(super) fn run() -> Result<String, CiError> {
    let allowed = match decide() {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "{}",
                crate::escape::stderr_line(&format!(
                    "no private-source token for this merge queue entry: {error}"
                ))
            );
            false
        }
    };
    let line = format!("allowed={allowed}\n");
    append_to(OUTPUT_VARIABLE, &line)?;
    Ok(line)
}
