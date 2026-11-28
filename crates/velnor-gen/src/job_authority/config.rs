// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{JOB_AUTHORITY_CONFIG_PATH, JOB_AUTHORITY_MAX_WORKFLOWS, RepoPath, RepoView};

use super::JobAuthorityError;

const MAX_WORKFLOWS: usize = JOB_AUTHORITY_MAX_WORKFLOWS;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    workflows: Vec<String>,
}

pub(super) fn workflows(view: &RepoView) -> Result<Option<Vec<RepoPath>>, JobAuthorityError> {
    let Some(bytes) = view.files.get(&JOB_AUTHORITY_CONFIG_PATH) else {
        return Ok(None);
    };
    let invalid = |reason: String| JobAuthorityError::Config(reason);
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8 text".to_owned()))?;
    let raw: Raw = toml::from_str(text).map_err(|error| invalid(error.message().to_owned()))?;
    if raw.workflows.len() > MAX_WORKFLOWS {
        return Err(JobAuthorityError::TooManyWorkflows {
            listed: raw.workflows.len(),
            limit: MAX_WORKFLOWS,
        });
    }
    let mut paths: Vec<RepoPath> = Vec::with_capacity(raw.workflows.len());
    for value in raw.workflows {
        let path = RepoPath::new(value.as_str())
            .map_err(|_| invalid(format!("workflows: {value:?} is not a repository path")))?;
        if paths.contains(&path) {
            return Err(invalid(format!("workflows: {value:?} is listed twice")));
        }
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(invalid("workflows must list at least one file".to_owned()));
    }
    Ok(Some(paths))
}
