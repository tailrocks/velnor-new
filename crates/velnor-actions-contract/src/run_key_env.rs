// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;

use crate::helper_error::HelperError;
use crate::ids::{run_key_for_ci, validate_run_key};

pub const PLAN_ATTEMPT_ENV: &str = "VELNOR_RUN_KEY_ATTEMPT";

pub fn resolve_run_key(explicit: Option<&str>) -> Result<String, HelperError> {
    if let Some(key) = explicit.filter(|key| !key.trim().is_empty()) {
        validate_run_key(key)?;
        return Ok(key.to_owned());
    }
    let id = env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty());
    let attempt = env::var(PLAN_ATTEMPT_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            env::var("GITHUB_RUN_ATTEMPT")
                .ok()
                .filter(|v| !v.is_empty())
        });
    let (Some(id), Some(attempt)) = (id, attempt) else {
        return Err(HelperError::internal("missing_run_key"));
    };
    let id: u64 = id
        .parse()
        .map_err(|_| HelperError::internal("bad_run_id"))?;
    let attempt: u64 = attempt
        .parse()
        .map_err(|_| HelperError::internal("bad_run_attempt"))?;
    Ok(run_key_for_ci(id, attempt))
}
