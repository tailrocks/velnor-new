// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{BTreeMap, GitRequest, Path, TrackedFile, fs, sha256, valid_git_sha, valid_path};

pub(super) fn git_output(
    root: &Path,
    request: &GitRequest,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<u8>, &'static str> {
    let result = request
        .run_release_in(root, git)
        .map_err(|_| "release_pr_checkout_git")?;
    if !result.success {
        return Err("release_pr_checkout_git");
    }
    Ok(result.stdout)
}

pub(super) fn tracked_files(
    root: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<BTreeMap<String, TrackedFile>, &'static str> {
    let output = git_output(
        root,
        &GitRequest::ls_tree(vec![
            "-r".into(),
            "-z".into(),
            "--full-tree".into(),
            "HEAD".into(),
        ]),
        git,
    )?;
    let mut files = BTreeMap::new();
    for entry in output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let record = std::str::from_utf8(entry).map_err(|_| "release_pr_checkout_invalid")?;
        let (header, path) = record
            .split_once('\t')
            .ok_or("release_pr_checkout_invalid")?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || fields[1] != "blob"
            || !valid_path(path)
            || !matches!(fields[0], "100644" | "100755" | "120000")
            || !valid_git_sha(fields[2])
            || files
                .insert(
                    path.to_owned(),
                    TrackedFile {
                        mode: fields[0].into(),
                        oid: fields[2].into(),
                    },
                )
                .is_some()
        {
            return Err("release_pr_checkout_invalid");
        }
    }
    Ok(files)
}

pub(super) fn untracked_files(
    root: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<String>, &'static str> {
    let output = git_output(
        root,
        &GitRequest::ls_files(vec!["--others".into(), "-z".into()]),
        git,
    )?;
    output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|bytes| {
            let path = std::str::from_utf8(bytes).map_err(|_| "release_pr_checkout_invalid")?;
            if !valid_path(path) {
                return Err("release_pr_checkout_invalid");
            }
            Ok(path.to_owned())
        })
        .collect()
}

pub(super) fn git_file_digests(
    root: &Path,
    verified_git: &crate::crate_release_git::ReleaseGit,
) -> Result<BTreeMap<String, String>, &'static str> {
    let git = root.join(".git");
    let metadata = fs::symlink_metadata(&git).map_err(|_| "release_pr_checkout_git")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("release_pr_checkout_git");
    }
    let mut hashes = BTreeMap::new();
    for path in ["HEAD", "config"] {
        let bytes = read_regular(&git, path, 64 * 1024 * 1024)?;
        hashes.insert(path.to_owned(), sha256(&bytes));
    }
    let index = git_output(
        root,
        &GitRequest::ls_files(vec!["--stage".into(), "-z".into()]),
        verified_git,
    )?;
    hashes.insert("index".to_owned(), sha256(&index));
    Ok(hashes)
}

pub(super) fn read_tracked(root: &Path, path: &str, mode: &str) -> Result<Vec<u8>, &'static str> {
    check_parent_dirs(root, path)?;
    let metadata =
        fs::symlink_metadata(root.join(path)).map_err(|_| "release_pr_checkout_changed")?;
    if mode == "120000" {
        if !metadata.file_type().is_symlink() {
            return Err("release_pr_checkout_changed");
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            return Ok(fs::read_link(root.join(path))
                .map_err(|_| "release_pr_checkout_changed")?
                .as_os_str()
                .as_bytes()
                .to_vec());
        }
        #[cfg(not(unix))]
        return Err("release_pr_checkout_changed");
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() || !mode_matches(&metadata, mode) {
        return Err("release_pr_checkout_changed");
    }
    read_regular(root, path, 256 * 1024 * 1024)
}

pub(super) fn check_parent_dirs(root: &Path, path: &str) -> Result<(), &'static str> {
    let mut parent = root.to_path_buf();
    let Some((dirs, _)) = path.rsplit_once('/') else {
        return Ok(());
    };
    for part in dirs.split('/') {
        parent.push(part);
        let metadata = fs::symlink_metadata(&parent).map_err(|_| "release_pr_checkout_changed")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("release_pr_checkout_changed");
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn mode_matches(metadata: &fs::Metadata, mode: &str) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    let executable = metadata.permissions().mode() & 0o111 != 0;
    (mode == "100755") == executable
}

#[cfg(not(unix))]
pub(super) fn mode_matches(_metadata: &fs::Metadata, _mode: &str) -> bool {
    false
}

pub(super) fn read_regular(root: &Path, path: &str, limit: u64) -> Result<Vec<u8>, &'static str> {
    match crate::safe_read::read_repo_bytes(root, path, limit)
        .map_err(|_| "release_pr_checkout_changed")?
    {
        crate::safe_read::RepoBytes::Bytes(bytes) => Ok(bytes),
        crate::safe_read::RepoBytes::Absent => Err("release_pr_checkout_changed"),
    }
}
