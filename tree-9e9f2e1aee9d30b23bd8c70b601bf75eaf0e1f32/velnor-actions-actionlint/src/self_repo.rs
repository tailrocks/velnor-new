//! Actionlint 1.7.12 rejects GitHub's `$/` self-repository action form.
//!
//! That form is valid (runner >= 2.336.0). The generated config ignores
//! only that diagnostic, and only on workflows that emit
//! `uses: $/.github/actions/`. Consumer ignore requests stay forbidden.

use std::collections::BTreeSet;

use crate::ActionlintError;
use crate::config::is_valid_workflow_path;

const USES_MARKER: &str = "uses: $/.github/actions/";

/// Actionlint message pattern for the `$/` false positive.
pub const SELF_REPO_ACTION_IGNORE: &str =
    "specifying action \"\\$/\\.github/actions/[^\"]+\" in invalid format because ref is missing";

/// Append the tool-gap ignore for workflows that call `$/.github/actions/`.
///
/// Workflows without that call, including every hosted-only render, keep
/// their bytes. A non-workflow file that contains the marker is an error:
/// actionlint would not see an ignore for a workflow that was mis-pathed.
///
/// # Errors
///
/// Returns [`ActionlintError::InvalidWorkflowPath`] when the marker sits
/// on a path that is not one `.github/workflows` file.
pub fn append_self_repo_action_gap(
    yaml: &str,
    files: &[(&str, &str)],
) -> Result<String, ActionlintError> {
    let paths = matching_workflows(files)?;
    if paths.is_empty() {
        return Ok(yaml.to_owned());
    }
    Ok(render_gap(yaml, &paths))
}

fn matching_workflows(files: &[(&str, &str)]) -> Result<BTreeSet<String>, ActionlintError> {
    let mut paths = BTreeSet::new();
    for (path, text) in files {
        if !text.contains(USES_MARKER) {
            continue;
        }
        if !is_valid_workflow_path(path) {
            return Err(ActionlintError::InvalidWorkflowPath {
                path: (*path).to_owned(),
            });
        }
        paths.insert((*path).to_owned());
    }
    Ok(paths)
}

fn render_gap(yaml: &str, paths: &BTreeSet<String>) -> String {
    let mut out = yaml.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("paths:\n");
    for path in paths {
        out.push_str("  ");
        out.push_str(path);
        out.push_str(":\n    ignore:\n      - '");
        out.push_str(SELF_REPO_ACTION_IGNORE);
        out.push_str("'\n");
    }
    out
}
