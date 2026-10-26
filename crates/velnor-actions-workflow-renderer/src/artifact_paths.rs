// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;

pub const CANDIDATE_OUTPUT_DIR_EXPR: &str = "${{ runner.temp }}/velnor/candidate-output";
pub const CANDIDATE_STAGE_DIR_EXPR: &str = "${{ runner.temp }}/velnor/candidate";
pub const PRESEED_OUTPUT_DIR_EXPR: &str = "${{ runner.temp }}/velnor/preseed-output";
pub const PRESEED_STAGE_DIR_EXPR: &str = "${{ runner.temp }}/velnor/preseed";

pub(crate) fn check_artifact_path(path: &str) -> Result<(), RenderError> {
    let mut rest = path;
    while let Some(at) = rest.find('$') {
        if !rest[at..].starts_with("${{") {
            return Err(RenderError::BadActionRef(format!(
                "shell_artifact_path:{path}"
            )));
        }
        rest = &rest[at + 3..];
    }
    Ok(())
}
