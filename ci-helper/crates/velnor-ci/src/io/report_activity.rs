// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::report::{INVOCATIONS, Invocation};
use std::fs;

pub(super) fn activity() -> Result<Option<crate::logscan::LogScan>, CiError> {
    let text = match fs::read_to_string(
        std::path::Path::new(&std::env::var("RUNNER_TEMP").unwrap_or_default()).join(INVOCATIONS),
    ) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CiError::new(error.to_string())),
    };
    let invocations = text
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|error| CiError::new(error.to_string())))
        .collect::<Result<Vec<Invocation>, _>>()?;
    if invocations.is_empty()
        || invocations
            .iter()
            .any(|invocation| invocation.progress.is_none())
    {
        return Ok(None);
    }
    let mut counts = crate::logscan::LogScan::default();
    for event in invocations
        .iter()
        .filter_map(|invocation| invocation.progress.as_ref())
        .flatten()
    {
        match event.kind {
            crate::cargo_artifacts::ProgressKind::Compiling => counts.compiling += 1,
            crate::cargo_artifacts::ProgressKind::Checking => counts.checking += 1,
            crate::cargo_artifacts::ProgressKind::Documenting => counts.documenting += 1,
            crate::cargo_artifacts::ProgressKind::Downloaded => counts.downloaded += 1,
        }
    }
    if !crate::cache::should_save(&counts)
        && invocations
            .iter()
            .any(|invocation| !invocation.capture_complete)
    {
        return Err(CiError::new(
            "output capture incomplete; cannot establish that no crates were compiled",
        ));
    }
    Ok(Some(counts))
}
