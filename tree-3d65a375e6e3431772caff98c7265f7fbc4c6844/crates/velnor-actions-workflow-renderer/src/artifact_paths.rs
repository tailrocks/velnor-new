//! GitHub-expression artifact paths for action `with:`/`env:` positions.
//!
//! Shell `$VAR` references never expand in action inputs or `env:` values:
//! only `${{ }}` expressions do. Every artifact transfer directory therefore
//! has two spellings: the `$RUNNER_TEMP` shell form for `run:` scripts and
//! the `${{ runner.temp }}` expression form here for `with: path:` and
//! `env:`. The path gate rejects shell-form stragglers fail-closed.

use crate::RenderError;

/// Expression form of the candidate output dir (`with: path:` only).
pub const CANDIDATE_OUTPUT_DIR_EXPR: &str = "${{ runner.temp }}/velnor/candidate-output";
/// Expression form of the candidate stage dir (`with: path:`/`env:` only).
pub const CANDIDATE_STAGE_DIR_EXPR: &str = "${{ runner.temp }}/velnor/candidate";
/// Expression form of the pre-seed output dir (`with: path:` only).
pub const PRESEED_OUTPUT_DIR_EXPR: &str = "${{ runner.temp }}/velnor/preseed-output";
/// Expression form of the pre-seed stage dir (`with: path:` only).
pub const PRESEED_STAGE_DIR_EXPR: &str = "${{ runner.temp }}/velnor/preseed";

/// Reject shell-form `$VAR` in artifact paths; only `${{ }}` expands there.
///
/// A `$RUNNER_TEMP` reference inside `with: path:` reaches the action
/// unexpanded, so uploads find no files and downloads land in a literal
/// workspace directory while later shell steps look under real runner
/// temp. Every `$` must open a `${{ }}` expression instead.
/// # Errors
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
