// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{GitHub, Repo};

#[derive(Deserialize)]
struct Repository {
    full_name: Option<String>,
    private: bool,
    visibility: Option<String>,
}

pub fn require_private(repository: &str, github: &dyn GitHub) -> Result<(), CiError> {
    let repo = Repo::parse(repository)?;
    let path = format!("/repos/{repo}");
    let text = github.get(&path)?;
    let answer: Repository = serde_json::from_str(&text)
        .map_err(|e| CiError::new(format!("GET {path} is not readable: {e}")))?;
    if answer
        .full_name
        .as_deref()
        .is_some_and(|name| !name.eq_ignore_ascii_case(repository))
    {
        return Err(CiError::new(format!(
            "GET {path} answered for {}, not for {repository}",
            answer.full_name.unwrap_or_default()
        )));
    }
    match (answer.private, answer.visibility.as_deref()) {
        (true, None | Some("private")) => Ok(()),
        (true, Some(visibility)) => Err(CiError::new(format!(
            "{repository} has the visibility {visibility}, and only a private repository can keep private dependencies in its caches"
        ))),
        (false, _) => Err(CiError::new(format!(
            "{repository} is not private, and a cache of a public repository can be read by a fork"
        ))),
    }
}
