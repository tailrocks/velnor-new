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

#[derive(Debug)]
pub enum Rejection {
    Unavailable(CiError),
    NotPrivate(CiError),
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(error) | Self::NotPrivate(error) => error.fmt(f),
        }
    }
}

pub fn require_private(repository: &str, github: &dyn GitHub) -> Result<(), Rejection> {
    let unavailable = |message: String| Rejection::Unavailable(CiError::new(message));
    let repo = Repo::parse(repository).map_err(Rejection::Unavailable)?;
    let path = format!("/repos/{repo}");
    let text = github.get(&path).map_err(Rejection::Unavailable)?;
    let answer: Repository = serde_json::from_str(&text)
        .map_err(|e| unavailable(format!("GET {path} is not readable: {e}")))?;
    let Some(name) = answer.full_name.as_deref() else {
        return Err(unavailable(format!(
            "GET {path} did not name the repository, so it cannot prove {repository} private"
        )));
    };
    if !name.eq_ignore_ascii_case(repository) {
        return Err(unavailable(format!(
            "GET {path} answered for {name}, not for {repository}"
        )));
    }
    let confirmed = |message: String| Rejection::NotPrivate(CiError::new(message));
    match (answer.private, answer.visibility.as_deref()) {
        (true, Some("private")) => Ok(()),
        (false, Some("public")) => Err(confirmed(format!(
            "{repository} is not private, and a cache of a public repository can be read by a fork"
        ))),
        (true, Some("internal")) => Err(confirmed(format!(
            "{repository} is internal, and only a private repository can keep private dependencies in its caches"
        ))),
        (private, visibility) => Err(unavailable(format!(
            "GET {path} answered private={private} with visibility {}, which does not confirm the visibility of {repository}",
            visibility.unwrap_or("(none)")
        ))),
    }
}
