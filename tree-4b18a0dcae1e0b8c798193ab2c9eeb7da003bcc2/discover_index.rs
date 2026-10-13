//! File-index construction: `git ls-files` plus an untracked pass in git
//! repositories, filesystem walk only for non-git roots.

use std::path::Path;

use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{FileIndex, build_index, build_index_from_list};

use crate::OrchestratorError;

/// Build the file index: `git ls-files` plus an untracked pass in git
/// repositories, filesystem walk only for non-git roots.
pub(crate) fn build_file_index(
    root: &Path,
    exclusions: &[String],
) -> Result<FileIndex, OrchestratorError> {
    if let Some(files) = git_file_list(root) {
        return build_index_from_list(root, &files, exclusions).map_err(|err| {
            OrchestratorError::Discovery {
                problem: err.to_string(),
            }
        });
    }
    build_index(root, exclusions).map_err(|err| OrchestratorError::Discovery {
        problem: err.to_string(),
    })
}

/// Tracked plus untracked repository-relative paths, or `None` for non-git.
fn git_file_list(root: &Path) -> Option<Vec<String>> {
    let tracked = ls_files(root, vec![std::ffi::OsString::from("-z")])?;
    let untracked = ls_files(
        root,
        ["--others", "--exclude-standard", "-z"]
            .iter()
            .map(std::ffi::OsString::from)
            .collect(),
    )?;
    Some(tracked.into_iter().chain(untracked).collect())
}

/// One `git ls-files -z` run; `None` when git fails or output is unusable.
fn ls_files(root: &Path, args: Vec<std::ffi::OsString>) -> Option<Vec<String>> {
    let output = GitRequest::ls_files(args).run_in(root).ok()?;
    if !output.success {
        return None;
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8(entry.to_vec()).ok())
        .collect()
}
