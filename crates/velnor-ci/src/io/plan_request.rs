// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::{append_outputs, open_regular, read_limited};
use crate::cli::{PlanArgs, PlanMode};
use crate::error::CiError;
use crate::plan_request::{Mode, RunSource, outputs};
use crate::write_request::internal;

const REQUEST_LIMIT: u64 = 8 << 20;

pub(super) fn run(args: &PlanArgs, mode: PlanMode, request: &Path) -> Result<String, CiError> {
    let text = read_request(request)?;
    let variable = |name: &str| std::env::var(name).ok();
    let (github_sha, run_id, override_attempt, attempt) = (
        variable("GITHUB_SHA"),
        variable("GITHUB_RUN_ID"),
        variable("VELNOR_RUN_KEY_ATTEMPT"),
        variable("GITHUB_RUN_ATTEMPT"),
    );
    let written = outputs(
        &text,
        match mode {
            PlanMode::Ci => Mode::Ci,
            PlanMode::Qualification => Mode::Qualification,
        },
        args.default_branch.as_deref(),
        &RunSource {
            github_sha: github_sha.as_deref(),
            run_id: run_id.as_deref(),
            attempt_override: override_attempt.as_deref(),
            run_attempt: attempt.as_deref(),
        },
        &super::current_dir()?,
    )?;
    append_outputs(&written)?;
    Ok(String::new())
}

fn read_request(path: &Path) -> Result<String, CiError> {
    let unreadable = || internal("unreadable_request");
    let file = open_regular(path).ok_or_else(unreadable)?;
    let bytes = read_limited(file, REQUEST_LIMIT).map_err(|_| unreadable())?;
    String::from_utf8(bytes).map_err(|_| unreadable())
}
