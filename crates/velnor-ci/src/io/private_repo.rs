// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{OUTPUT_VARIABLE, append_to};
use crate::error::CiError;

const REPOSITORY_VARIABLE: &str = "GITHUB_REPOSITORY";
const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";
const VERDICT_VARIABLE: &str = "VELNOR_REPOSITORY_PRIVATE";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(30);

fn variable(name: &str) -> Result<String, CiError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CiError::new(format!("{name} is not set")))
}

pub(super) fn require() -> Result<(), CiError> {
    let repository = variable(REPOSITORY_VARIABLE)?;
    let github = super::fetch::api_client(variable(TOKEN_VARIABLE)?, LOOKUP_TIMEOUT)?;
    crate::private_repo::require_private(&repository, &github)
}

pub(super) fn confirmed() -> Result<(), CiError> {
    match std::env::var(VERDICT_VARIABLE).as_deref() {
        Ok("true") => Ok(()),
        _ => Err(CiError::new(
            "the repository was not confirmed private before the build",
        )),
    }
}

pub(super) fn run() -> Result<String, CiError> {
    let private = match require() {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "{}",
                crate::escape::stderr_line(&format!(
                    "the base caches of private dependencies are not used: {error}"
                ))
            );
            false
        }
    };
    let line = format!("private={private}\n");
    append_to(OUTPUT_VARIABLE, &line)?;
    Ok(line)
}
