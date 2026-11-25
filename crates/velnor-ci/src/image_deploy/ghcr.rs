// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{GitHub, parse};

#[derive(Deserialize)]
struct Package {
    visibility: Option<String>,
}

fn package_path(scope: &str, owner: &str, name: &str) -> String {
    format!(
        "/{scope}/{owner}/packages/container/{}",
        name.replace('/', "%2F")
    )
}

pub fn preflight(github: &dyn GitHub, repository: &str) -> Result<String, CiError> {
    let rest = repository.strip_prefix("ghcr.io/").unwrap_or_default();
    let valid = rest.split_once('/').is_some_and(|(owner, name)| {
        !owner.is_empty()
            && !name.is_empty()
            && rest.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"./_-".contains(&byte)
            })
    });
    let (owner, name) = rest
        .split_once('/')
        .filter(|_| valid)
        .ok_or_else(|| CiError::new(format!("`{repository}` is not ghcr.io/owner/name")))?;
    for scope in ["orgs", "users"] {
        match github.get(&package_path(scope, owner, name)) {
            Ok(body) => {
                let package: Package = parse(&body, "package")?;
                return match package.visibility.as_deref() {
                    Some("public") => Err(CiError::new(format!(
                        "{repository} is a public package, and GitHub cannot make a public package private again; pushing a private image there would publish it, so use another package name or delete the package first"
                    ))),
                    Some(other) => Ok(format!("{repository} exists and is {other}\n")),
                    None => Err(CiError::new(format!(
                        "GitHub answered the package lookup of {repository} without a visibility"
                    ))),
                };
            }
            Err(error) if error.is_missing() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(format!(
        "{repository} does not exist yet; the first push creates it, private by default for a container package\n"
    ))
}
