//! File-index construction: `git ls-files` plus an untracked pass in git
//! repositories, filesystem walk only for non-git roots.

use std::path::Path;

use velnor_actions_contract::{
    FileIndex, build_index, build_index_from_list, is_reserved_cache_path_bytes,
};
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
        tracked: GitPathList,
        untracked: GitPathList,
    },
}

/// One `git ls-files -z` result with raw cache identity retained.
struct GitPathList {
    paths: Vec<String>,
    skipped_non_utf8: bool,
    reserved_cache_path: Option<String>,
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
    if let GitFiles::Files { tracked, untracked } = git_file_list(root) {
        if let Some(path) = tracked.reserved_cache_path {
            return Err(OrchestratorError::Discovery {
                problem: format!("tracked_reserved_cache_path:{path}"),
            });
        }
        let skipped_non_utf8 = tracked.skipped_non_utf8 || untracked.skipped_non_utf8;
        let files: Vec<String> = tracked.paths.into_iter().chain(untracked.paths).collect();
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

/// Tracked and untracked repository-relative paths; `NonRepo` when git
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
    GitFiles::Files { tracked, untracked }
}

/// Match the reserved cache root and descendants, not similarly named paths.
/// One `git ls-files -z` run; `None` only when git fails or is absent.
/// Output always splits: undecodable entries skip with an explicit flag.
fn ls_files(root: &Path, args: Vec<std::ffi::OsString>) -> Option<GitPathList> {
    let output = GitRequest::ls_files(args).run_in(root).ok()?;
    if !output.success {
        return None;
    }
    let reserved_cache_path = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .find(|path| is_reserved_cache_path_bytes(path))
        .map(|path| String::from_utf8_lossy(path).into_owned());
    let (paths, skipped_non_utf8) = split_nul_paths_skipping(&output.stdout);
    Some(GitPathList {
        paths,
        skipped_non_utf8,
        reserved_cache_path,
    })
}

#[cfg(test)]
#[path = "discover_index_cache_tests.rs"]
mod cache_tests;
