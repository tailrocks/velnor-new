//! File-index construction: `git ls-files` plus an untracked pass in git
//! repositories, filesystem walk only for non-git roots.

use std::path::Path;

use velnor_actions_contract::{FileIndex, build_index, build_index_from_list};
use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;
use crate::git_paths::split_nul_paths_skipping;

/// `git ls-files` outcome: non-git roots walk the filesystem, while
/// undecodable entries skip with an explicit flag (never a silent
/// fallback that would re-encounter the same names in the walk).
enum GitFiles {
    /// Not a git repository (or git unusable): walk the filesystem.
    NonRepo,
    /// Enumerated paths plus whether any entry was skipped.
    Files {
        files: Vec<String>,
        skipped_non_utf8: bool,
    },
}

/// Build the file index plus whether any name was skipped.
///
/// `git ls-files` plus an untracked pass in git repositories,
/// filesystem walk only for non-git roots. Both paths skip
/// non-UTF-8 names and report them through the flag; selection
/// broadens explicitly on it.
pub(crate) fn build_file_index(
    root: &Path,
    exclusions: &[String],
) -> Result<(FileIndex, bool), OrchestratorError> {
    if let GitFiles::Files {
        files,
        skipped_non_utf8,
    } = git_file_list(root)
    {
        let index = build_index_from_list(root, &files, exclusions).map_err(|err| {
            OrchestratorError::Discovery {
                problem: err.to_string(),
            }
        })?;
        return Ok((index, skipped_non_utf8));
    }
    let index = build_index(root, exclusions).map_err(|err| OrchestratorError::Discovery {
        problem: err.to_string(),
    })?;
    let skipped = index.skipped_non_utf8();
    Ok((index, skipped))
}

/// Tracked plus untracked repository-relative paths; `NonRepo` when git
/// itself is unavailable. Undecodable entries skip with an explicit flag.
fn git_file_list(root: &Path) -> GitFiles {
    let tracked = ls_files(root, vec![std::ffi::OsString::from("-z")]);
    let untracked = ls_files(
        root,
        ["--others", "--exclude-standard", "-z"]
            .iter()
            .map(std::ffi::OsString::from)
            .collect(),
    );
    let (Some(tracked), Some(untracked)) = (tracked, untracked) else {
        return GitFiles::NonRepo;
    };
    let skipped = tracked.1 || untracked.1;
    let files = tracked.0.into_iter().chain(untracked.0).collect();
    GitFiles::Files {
        files,
        skipped_non_utf8: skipped,
    }
}

/// One `git ls-files -z` run; `None` only when git fails or is absent.
/// Output always splits: undecodable entries skip with an explicit flag.
fn ls_files(root: &Path, args: Vec<std::ffi::OsString>) -> Option<(Vec<String>, bool)> {
    let output = GitRequest::ls_files(args).run_in(root).ok()?;
    if !output.success {
        return None;
    }
    Some(split_nul_paths_skipping(&output.stdout))
}
