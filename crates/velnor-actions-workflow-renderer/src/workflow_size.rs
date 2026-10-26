// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use std::path::Path;

pub const MAX_WORKFLOW_BYTES: usize = 500_000;

pub(crate) fn check_workflow_size(path: &str, bytes: &str) -> Result<(), RenderError> {
    if !is_workflow_path(path) || bytes.len() <= MAX_WORKFLOW_BYTES {
        return Ok(());
    }
    Err(RenderError::InvalidWorkflow(format!(
        "workflow_too_large:{path}:{}:{MAX_WORKFLOW_BYTES}",
        bytes.len()
    )))
}

pub(crate) fn is_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && Path::new(path).extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        })
}
