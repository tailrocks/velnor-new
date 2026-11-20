// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};

const EVERY_PATH: &str = "**";

pub(super) fn names(
    file: &str,
    what: &str,
    values: Vec<String>,
    allowed: &[&str],
) -> Result<Vec<String>, WorkflowsError> {
    for (index, value) in values.iter().enumerate() {
        if !allowed.contains(&value.as_str()) || values[..index].contains(value) {
            return refuse(format!(
                "{what} of {file} has {value:?}, which is not a unique one of {allowed:?}"
            ));
        }
    }
    Ok(values)
}

pub(super) fn workflow_names(
    file: &str,
    values: Vec<String>,
) -> Result<Vec<String>, WorkflowsError> {
    for (index, value) in values.iter().enumerate() {
        let bad = value.trim().is_empty()
            || value.contains(char::is_control)
            || value.contains("${{")
            || values[..index].contains(value);
        if bad {
            return refuse(format!(
                "workflow_run workflows of {file} has {value:?}, which is not a unique one-line workflow name without expressions"
            ));
        }
    }
    Ok(values)
}

pub(super) fn branch_patterns(
    file: &str,
    what: &str,
    values: Vec<String>,
) -> Result<Vec<String>, WorkflowsError> {
    if values.iter().any(|value| value == EVERY_PATH) {
        return refuse(format!(
            "{what} of {file} has {EVERY_PATH:?}, which is redundant; omit branches to match every branch"
        ));
    }
    filter_patterns(file, what, values)
}

pub(super) fn filter_patterns(
    file: &str,
    what: &str,
    values: Vec<String>,
) -> Result<Vec<String>, WorkflowsError> {
    if !values.is_empty() && values.iter().all(|value| value.starts_with('!')) {
        return refuse(format!(
            "{what} of {file} has only negative patterns; GitHub requires at least one pattern that does not start with '!'"
        ));
    }
    patterns(file, what, values)
}

pub(super) type PathFilters = (Vec<String>, Vec<String>);

pub(super) fn path_filters(
    file: &str,
    event: &str,
    paths: Vec<String>,
    ignored: Vec<String>,
) -> Result<PathFilters, WorkflowsError> {
    if !paths.is_empty() && !ignored.is_empty() {
        return refuse(format!(
            "the {event} trigger of {file} sets both paths and paths_ignore; GitHub allows one of them per trigger"
        ));
    }
    if paths.iter().any(|value| value == EVERY_PATH) {
        return refuse(format!(
            "{event} paths of {file} has {EVERY_PATH:?}, which is redundant; omit paths to run on every change"
        ));
    }
    if ignored.iter().any(|value| value == EVERY_PATH) {
        return refuse(format!(
            "{event} paths_ignore of {file} has {EVERY_PATH:?}, which ignores every file, so the workflow could never run"
        ));
    }
    Ok((
        filter_patterns(file, &format!("{event} paths"), paths)?,
        patterns(file, &format!("{event} paths_ignore"), ignored)?,
    ))
}

pub(super) fn patterns(
    file: &str,
    what: &str,
    values: Vec<String>,
) -> Result<Vec<String>, WorkflowsError> {
    for (index, value) in values.iter().enumerate() {
        let bad = value.is_empty()
            || value.contains(char::is_whitespace)
            || value.contains(char::is_control)
            || value.contains("${{")
            || values[..index].contains(value);
        if bad {
            return refuse(format!(
                "{what} of {file} has {value:?}, which is not a unique pattern without spaces or expressions"
            ));
        }
    }
    Ok(values)
}
