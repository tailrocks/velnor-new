use crate::{OrchestratorError, internal::internal};
use std::path::Path;
use velnor_actions_mise::{CheckDeadline, GitRequest};

pub(super) fn require_tracked_clean_input(
    root: &Path,
    input: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let listed = GitRequest::ls_files(vec![
        "--error-unmatch".into(),
        "--stage".into(),
        "--".into(),
        input.into(),
    ])
    .run_in_until(root, deadline)
    .map_err(|error| internal(&error.to_string()))?;
    if !listed.success || !valid_stage_zero_regular_file(&listed.stdout, input) {
        return Err(internal("preview_input_not_tracked_regular"));
    }
    let changed = GitRequest::diff(vec![
        "--quiet".into(),
        "HEAD".into(),
        "--".into(),
        input.into(),
    ])
    .run_in_until(root, deadline)
    .map_err(|error| internal(&error.to_string()))?;
    if !changed.success {
        return Err(internal("preview_input_not_source_clean"));
    }
    let untracked = GitRequest::ls_files(vec![
        "--others".into(),
        "--exclude-standard".into(),
        "--".into(),
        input.into(),
    ])
    .run_in_until(root, deadline)
    .map_err(|error| internal(&error.to_string()))?;
    if !untracked.success || !untracked.stdout.is_empty() {
        return Err(internal("preview_input_untracked_content"));
    }
    Ok(())
}

pub(super) fn valid_stage_zero_regular_file(output: &[u8], expected: &str) -> bool {
    let Ok(text) = std::str::from_utf8(output) else {
        return false;
    };
    let mut lines = text.lines();
    let Some(line) = lines.next() else {
        return false;
    };
    if lines.next().is_some() {
        return false;
    }
    let Some((metadata, path)) = line.split_once('\t') else {
        return false;
    };
    let mut fields = metadata.split_ascii_whitespace();
    matches!(fields.next(), Some("100644" | "100755"))
        && fields.next().is_some_and(|object| {
            matches!(object.len(), 40 | 64)
                && object
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        && fields.next() == Some("0")
        && fields.next().is_none()
        && path == expected
}
