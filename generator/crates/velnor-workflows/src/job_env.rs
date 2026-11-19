// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::{WorkflowsError, refuse};
use crate::expression::{ENV_INTERPOLATIONS, check_interpolated, is_identifier};
use crate::plan::{CacheMode, Env};

const WRITER_GATE: &str =
    "github.ref != format('refs/heads/{0}', github.event.repository.default_branch)";

pub(crate) fn job_env(
    job: &str,
    raw: BTreeMap<String, String>,
) -> Result<Vec<(String, Env)>, WorkflowsError> {
    let mut items = Vec::new();
    for (key, value) in raw {
        if !is_identifier(&key, false) {
            return refuse(format!(
                "job {job} env name {key:?} is not a variable name of letters, digits and underscores"
            ));
        }
        if value.contains("${{") {
            check_interpolated(&format!("job {job} env {key}"), &value, &ENV_INTERPOLATIONS)?;
            items.push((key, Env::Interpolated(value)));
        } else {
            items.push((key, Env::Text(value)));
        }
    }
    Ok(items)
}

pub(crate) fn cache_mode(
    job: &str,
    raw: Option<&str>,
    conditions: &[Option<&str>],
) -> Result<Option<CacheMode>, WorkflowsError> {
    let Some(name) = raw else {
        return Ok(None);
    };
    let Some(mode) = CacheMode::parse(name) else {
        return refuse(format!(
            "job {job} cache_mode {name:?} must be one of {:?}",
            CacheMode::NAMES
        ));
    };
    let gated = conditions
        .iter()
        .flatten()
        .any(|condition| condition.contains(WRITER_GATE));
    if mode.writes() && !gated {
        return refuse(format!(
            "job {job} cache_mode {name:?} lets the job write the cache, so its if (or the workflow if) must contain {WRITER_GATE} to keep it off the default branch"
        ));
    }
    Ok(Some(mode))
}
