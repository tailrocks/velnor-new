use std::path::Path;

use velnor_actions_contract::{
    FileIndex, build_index, build_index_from_list, is_reserved_cache_path_bytes,
};
use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;
use crate::git_paths::split_nul_paths_skipping;

enum GitFiles {
    NonRepo,
    Files {
        tracked: GitPathList,
        untracked: GitPathList,
    },
}

struct GitPathList {
    paths: Vec<String>,
    skipped_non_utf8: bool,
    reserved_cache_path: Option<String>,
}

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
