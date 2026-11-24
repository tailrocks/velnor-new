// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{OUTPUT_VARIABLE, append_to};
use crate::error::CiError;
use crate::private_repo::Rejection;
use crate::public_probe::Deadline;

const API_VARIABLE: &str = "VELNOR_API_URL";
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

pub(super) fn require() -> Result<(), Rejection> {
    let repository = variable(REPOSITORY_VARIABLE).map_err(Rejection::Unavailable)?;
    let token = variable(TOKEN_VARIABLE).map_err(Rejection::Unavailable)?;
    let github = super::fetch::api_client(token, LOOKUP_TIMEOUT).map_err(Rejection::Unavailable)?;
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
    let (private, unavailable) = match require() {
        Ok(()) => (true, false),
        Err(error) => {
            eprintln!(
                "{}",
                crate::escape::stderr_line(&format!(
                    "the base caches of private dependencies are not used: {error}"
                ))
            );
            (false, matches!(error, Rejection::Unavailable(_)))
        }
    };
    let line = if unavailable {
        "private=false\nunavailable=true\n".to_owned()
    } else {
        format!("private={private}\n")
    };
    append_to(OUTPUT_VARIABLE, &line)?;
    Ok(line)
}

fn still_private() -> Result<bool, CiError> {
    let repository = variable(REPOSITORY_VARIABLE)?;
    let (owner, name) = repository
        .split_once('/')
        .filter(|(owner, name)| !owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .ok_or_else(|| CiError::new(format!("{REPOSITORY_VARIABLE} is not owner/name")))?;
    let api = variable(API_VARIABLE)
        .ok()
        .filter(|api| super::http::trusted_api(api))
        .ok_or_else(|| CiError::new(format!("set {API_VARIABLE} to github.api_url")))?;
    let target = velnor_model::GitRepository {
        owner: owner.to_owned(),
        name: name.to_owned(),
    };
    match super::public_repo::check(&api, &target, None, Deadline::after(LOOKUP_TIMEOUT)) {
        Err(super::public_repo::Rejection::Private(_)) => Ok(true),
        Ok(_) => Ok(false),
        Err(super::public_repo::Rejection::Unproven(reason)) => Err(CiError::new(reason)),
    }
}

pub(super) fn recheck() -> Result<String, CiError> {
    let line = match still_private() {
        Ok(true) => "private=true\n".to_owned(),
        Ok(false) => {
            eprintln!(
                "the repository can be read without a token, so its private caches are not saved"
            );
            "private=false\n".to_owned()
        }
        Err(error) => {
            eprintln!(
                "{}",
                crate::escape::stderr_line(&format!(
                    "the private caches are not saved because the repository is not proven non-public: {error}"
                ))
            );
            "private=false\nunavailable=true\n".to_owned()
        }
    };
    append_to(OUTPUT_VARIABLE, &line)?;
    Ok(line)
}
