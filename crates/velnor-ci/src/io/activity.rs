// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::telemetry::{INVOCATIONS, Invocation};
use std::collections::BTreeSet;
use std::fs;

pub(super) fn invocations() -> Result<Option<Vec<Invocation>>, CiError> {
    let text = match fs::read_to_string(
        std::path::Path::new(&std::env::var("RUNNER_TEMP").unwrap_or_default()).join(INVOCATIONS),
    ) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CiError::new(error.to_string())),
    };
    text.lines()
        .map(|line| serde_json::from_str(line).map_err(|error| CiError::new(error.to_string())))
        .collect::<Result<Vec<Invocation>, _>>()
        .map(Some)
}

pub(super) fn activity(
    invocations: Option<&[Invocation]>,
) -> Result<Option<crate::logscan::LogScan>, CiError> {
    let Some(invocations) = invocations else {
        return Ok(None);
    };
    if invocations.is_empty()
        || invocations
            .iter()
            .any(|invocation| invocation.progress.is_none())
    {
        return Ok(None);
    }
    let mut counts = crate::logscan::LogScan::default();
    for invocation in invocations {
        let before = counts.crates_compiled();
        for event in invocation.progress.iter().flatten() {
            match event.kind {
                crate::cargo_artifacts::ProgressKind::Compiling => counts.compiling += 1,
                crate::cargo_artifacts::ProgressKind::Checking => counts.checking += 1,
                crate::cargo_artifacts::ProgressKind::Documenting => counts.documenting += 1,
                crate::cargo_artifacts::ProgressKind::Downloaded => counts.downloaded += 1,
            }
        }
        let artifacts = invocation
            .artifacts
            .iter()
            .filter(|artifact| !artifact.fresh)
            .map(|artifact| &artifact.package_id)
            .collect::<BTreeSet<_>>()
            .len() as u64;
        counts.compiling += artifacts.saturating_sub(counts.crates_compiled() - before);
    }
    if !crate::cache::should_save(&counts)
        && invocations
            .iter()
            .any(|invocation| !invocation.capture_complete || invocation.rejected > 0)
    {
        return Err(CiError::new(
            "output capture incomplete or rejected records; cannot establish that no crates were compiled",
        ));
    }
    Ok(Some(counts))
}
