// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_git::ReleaseGit;
use crate::{OrchestratorError, internal::internal};
use std::{fs, path::Path};
use velnor_actions_mise::GitRequest;

pub fn verify_journal_source(
    root: &Path,
    receipt: &crate::crate_release_journal::JournalReceipt,
    git: &ReleaseGit,
) -> Result<(), OrchestratorError> {
    if journal_source_inventory(root, &receipt.identity.source_sha, git)? != receipt.source_files {
        return Err(internal("crate_release_journal_source_mismatch"));
    }
    Ok(())
}

pub fn journal_source_inventory(
    root: &Path,
    expected_sha: &str,
    git: &ReleaseGit,
) -> Result<Vec<crate::crate_release_journal::JournalFile>, OrchestratorError> {
    use crate::crate_release_journal::{JournalFile, JournalFileKind};
    verify_crate_release_source(root, expected_sha, git)?;
    let entries = source_tree_entries(root, git)?;
    if entries.is_empty() || entries.len() > 100_000 {
        return Err(internal("crate_release_journal_inventory_invalid"));
    }
    let anchor = crate::crate_release_inventory_read::open_root(root)?;
    let mut total_bytes = 0_u64;
    let mut files = Vec::with_capacity(entries.len());
    for (path, mode, object) in entries {
        let (size, sha256, target) =
            crate::crate_release_inventory_read::read_inventory(&anchor, &path, &mode, &object)?;
        let kind = match (mode.as_str(), target) {
            ("100644" | "100755", None) => JournalFileKind::Regular {
                executable: mode == "100755",
            },
            ("120000", Some(target)) => JournalFileKind::Symlink { target },
            _ => return Err(internal("crate_release_source_file_type_unsupported")),
        };
        total_bytes = total_bytes
            .checked_add(size)
            .ok_or_else(|| internal("crate_release_journal_inventory_invalid"))?;
        if total_bytes > 2 * 1024 * 1024 * 1024 {
            return Err(internal("crate_release_journal_inventory_invalid"));
        }
        files.push(JournalFile { path, sha256, kind });
    }
    if !crate::crate_release_journal::valid_source_files(&files) {
        return Err(internal("crate_release_journal_inventory_invalid"));
    }
    verify_crate_release_source(root, expected_sha, git)?;
    Ok(files)
}

pub fn verify_crate_release_source(
    root: &Path,
    expected_sha: &str,
    git: &ReleaseGit,
) -> Result<(), OrchestratorError> {
    if expected_sha.len() != 40
        || !expected_sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(internal("crate_release_source_sha_invalid"));
    }
    let metadata =
        fs::symlink_metadata(root).map_err(|_| internal("crate_release_source_unavailable"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(internal("crate_release_source_root_invalid"));
    }
    let canonical = root
        .canonicalize()
        .map_err(|_| internal("crate_release_source_root_invalid"))?;
    let top = GitRequest::rev_parse(vec!["--show-toplevel".into()])
        .run_release_in(&canonical, git)
        .map_err(|_| internal("crate_release_source_git"))?;
    if !top.success {
        return Err(internal("crate_release_source_git"));
    }
    let top = top
        .stdout_text("git")
        .map_err(|_| internal("crate_release_source_git"))?;
    let top = Path::new(top.trim())
        .canonicalize()
        .map_err(|_| internal("crate_release_source_git"))?;
    let head = GitRequest::rev_parse(vec!["HEAD".into()])
        .run_release_in(&canonical, git)
        .map_err(|_| internal("crate_release_source_git"))?;
    let head_sha = head
        .stdout_text("git")
        .map_err(|_| internal("crate_release_source_git"))?;
    if top != canonical || !head.success || head_sha.trim() != expected_sha {
        return Err(internal("crate_release_source_mismatch"));
    }
    validate_source_links(&canonical, git)?;
    let untracked = GitRequest::ls_files(vec!["--others".into()])
        .run_release_in(&canonical, git)
        .map_err(|_| internal("crate_release_source_git"))?;
    if !untracked.success || !untracked.stdout.is_empty() {
        return Err(internal("crate_release_source_untracked"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleaseSourcePackage {
    pub name: String,
    pub version: String,
    pub manifest: String,
}

pub(crate) fn source_tool_catalog(
    rust: &str,
) -> Result<velnor_actions_mise::ToolCatalog, velnor_actions_mise::MiseError> {
    use velnor_actions_mise::{PinnedTool, ToolCatalog};
    let baseline = ToolCatalog::pinned();
    ToolCatalog::new(
        rust,
        baseline.version(PinnedTool::MrBoxington),
        baseline.version(PinnedTool::Gh),
        baseline.version(PinnedTool::Jactionlint),
        baseline.version(PinnedTool::Shellcheck),
        baseline.version(PinnedTool::Nextest),
        baseline.version(PinnedTool::Opentofu),
    )
}

fn validate_source_links(root: &Path, git: &ReleaseGit) -> Result<(), OrchestratorError> {
    use std::collections::BTreeSet;
    if !cfg!(unix) {
        return Err(internal("crate_release_source_platform_unsupported"));
    }
    let output = GitRequest::ls_files(vec!["--stage".into(), "-z".into()])
        .run_release_in(root, git)
        .map_err(|_| internal("crate_release_source_git"))?;
    if !output.success {
        return Err(internal("crate_release_source_git"));
    }
    let mut tree_entries = source_tree_entries(root, git)?;
    let mut files = BTreeSet::new();
    let mut links = Vec::new();
    for entry in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let record = std::str::from_utf8(entry)
            .map_err(|_| internal("crate_release_source_path_unsupported"))?;
        let (header, path) = record
            .split_once('\t')
            .ok_or_else(|| internal("crate_release_source_git"))?;
        let fields: Vec<&str> = header.split(' ').collect();
        if fields.len() != 3
            || fields[2] != "0"
            || path.is_empty()
            || !Path::new(path)
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
        {
            return Err(internal("crate_release_source_path_invalid"));
        }
        if !tree_entries.remove(&(path.to_owned(), fields[0].to_owned(), fields[1].to_owned())) {
            return Err(internal("crate_release_source_index_mismatch"));
        }
        verify_indexed_source_file(root, path, &fields, &mut files, &mut links)?;
    }
    if !tree_entries.is_empty() {
        return Err(internal("crate_release_source_index_mismatch"));
    }
    validate_link_targets(root, &files, links)?;
    Ok(())
}

fn source_tree_entries(
    root: &Path,
    git: &ReleaseGit,
) -> Result<std::collections::BTreeSet<(String, String, String)>, OrchestratorError> {
    let tree = GitRequest::ls_tree(vec![
        "-r".into(),
        "-z".into(),
        "--full-tree".into(),
        "HEAD".into(),
    ])
    .run_release_in(root, git)
    .map_err(|_| internal("crate_release_source_git"))?;
    if !tree.success {
        return Err(internal("crate_release_source_git"));
    }
    let mut tree_entries = std::collections::BTreeSet::new();
    for entry in tree
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let record = std::str::from_utf8(entry)
            .map_err(|_| internal("crate_release_source_path_unsupported"))?;
        let (header, path) = record
            .split_once('\t')
            .ok_or_else(|| internal("crate_release_source_git"))?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || fields[1] != "blob"
            || !tree_entries.insert((path.to_owned(), fields[0].to_owned(), fields[2].to_owned()))
        {
            return Err(internal("crate_release_source_git"));
        }
    }
    Ok(tree_entries)
}

fn verify_indexed_source_file(
    root: &Path,
    path: &str,
    fields: &[&str],
    files: &mut std::collections::BTreeSet<String>,
    links: &mut Vec<String>,
) -> Result<(), OrchestratorError> {
    match fields[0] {
        "100644" | "100755" => {
            let metadata = fs::symlink_metadata(root.join(path))
                .map_err(|_| internal("crate_release_source_file_invalid"))?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || !executable_mode_matches(&metadata, fields[0])
            {
                return Err(internal("crate_release_source_file_invalid"));
            }
            let bytes = match crate::safe_read::read_repo_bytes(root, path, 256 * 1024 * 1024)? {
                crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
                crate::safe_read::RepoBytes::Absent => {
                    return Err(internal("crate_release_source_file_missing"));
                }
            };
            if crate::release_policy_admission::git_blob_sha1(&bytes)? != fields[1] {
                return Err(internal("crate_release_source_content_mismatch"));
            }
            files.insert(path.to_owned());
        }
        "120000" => {
            let target = fs::read_link(root.join(path))
                .map_err(|_| internal("crate_release_source_link_invalid"))?;
            let bytes = link_bytes(&target);
            if crate::release_policy_admission::git_blob_sha1(&bytes)? != fields[1] {
                return Err(internal("crate_release_source_link_invalid"));
            }
            links.push(path.to_owned());
        }
        _ => return Err(internal("crate_release_source_file_type_unsupported")),
    }
    Ok(())
}

fn validate_link_targets(
    root: &Path,
    files: &std::collections::BTreeSet<String>,
    links: Vec<String>,
) -> Result<(), OrchestratorError> {
    for link in links {
        let target = root
            .join(link)
            .canonicalize()
            .map_err(|_| internal("crate_release_source_link_invalid"))?;
        let relative = target
            .strip_prefix(root)
            .map_err(|_| internal("crate_release_source_link_escape"))?;
        let name = relative
            .to_str()
            .ok_or_else(|| internal("crate_release_source_path_unsupported"))?;
        if !target.is_file() || !files.contains(name) {
            return Err(internal("crate_release_source_link_untracked"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn executable_mode_matches(metadata: &std::fs::Metadata, mode: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    (metadata.permissions().mode() & 0o111 != 0) == (mode == "100755")
}
#[cfg(not(unix))]
fn executable_mode_matches(_metadata: &std::fs::Metadata, _mode: &str) -> bool {
    false
}

#[cfg(unix)]
fn link_bytes(target: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    target.as_os_str().as_bytes().to_vec()
}
#[cfg(not(unix))]
fn link_bytes(_target: &Path) -> Vec<u8> {
    Vec::new()
}

pub(crate) fn verify_toolchain_source_separation(source: &Path) -> Result<(), OrchestratorError> {
    let home = std::env::var_os("RUSTUP_HOME")
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    let home = Path::new(&home)
        .canonicalize()
        .map_err(|_| internal("crate_release_toolchain_invalid"))?;
    let source = source
        .canonicalize()
        .map_err(|_| internal("crate_release_source_root_invalid"))?;
    if home.starts_with(&source) || source.starts_with(&home) {
        return Err(internal("crate_release_toolchain_source_overlap"));
    }
    Ok(())
}

pub(crate) fn source_compiler_env(
    version: &str,
) -> Result<Vec<std::ffi::OsString>, OrchestratorError> {
    let home = std::env::var_os("RUSTUP_HOME")
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    let home = Path::new(&home);
    let rustc = verified_rust_tool_path(home, version, "rustc")?;
    let rustdoc = verified_rust_tool_path(home, version, "rustdoc")?;
    let bin = rustc
        .parent()
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    Ok(vec![
        format!("PATH={}:{}", bin.display(), "/usr/bin:/bin").into(),
        format!("RUSTC={}", rustc.display()).into(),
        format!("RUSTDOC={}", rustdoc.display()).into(),
    ])
}

pub(crate) fn source_cargo_path(
    rust: &str,
    _cwd: &Path,
) -> Result<std::path::PathBuf, OrchestratorError> {
    let home = std::env::var_os("RUSTUP_HOME")
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    verified_rust_tool_path(Path::new(&home), rust, "cargo")
}

pub fn verified_rust_tool_path(
    rustup_home: &Path,
    version: &str,
    tool: &str,
) -> Result<std::path::PathBuf, OrchestratorError> {
    if !rustup_home.is_absolute()
        || !matches!(tool, "cargo" | "rustc" | "rustdoc")
        || !velnor_actions_contract::config::is_exact_semver(version)
    {
        return Err(internal("crate_release_toolchain_invalid"));
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Err(internal("crate_release_toolchain_platform_unsupported")),
    };
    let path = rustup_home
        .join("toolchains")
        .join(format!("{version}-{target}"))
        .join("bin")
        .join(tool);
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Normal(_) => {
                current.push(component);
            }
            _ => return Err(internal("crate_release_toolchain_invalid")),
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| internal("crate_release_toolchain_invalid"))?;
        if metadata.file_type().is_symlink() {
            return Err(internal("crate_release_toolchain_invalid"));
        }
        if current != path && !metadata.is_dir() {
            return Err(internal("crate_release_toolchain_invalid"));
        }
        if current == path && !metadata.is_file() {
            return Err(internal("crate_release_toolchain_invalid"));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&path)
            .map_err(|_| internal("crate_release_toolchain_invalid"))?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(internal("crate_release_toolchain_invalid"));
        }
    }
    Ok(path)
}

pub fn verify_owned_tool_homes(
    runner_temp: &Path,
    rustup_home: &Path,
    mise_rustup_home: &Path,
    cargo_home: &Path,
    mise_cargo_home: &Path,
    source: &Path,
) -> Result<(), OrchestratorError> {
    if !runner_temp.is_absolute()
        || rustup_home != runner_temp.join("velnor/rustup")
        || mise_rustup_home != rustup_home
        || cargo_home != runner_temp.join("velnor/cargo")
        || mise_cargo_home != cargo_home
    {
        return Err(internal("crate_release_owned_homes_invalid"));
    }
    for path in [runner_temp, rustup_home, cargo_home, source] {
        let mut current = std::path::PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::RootDir | std::path::Component::Normal(_) => {
                    current.push(component);
                }
                _ => return Err(internal("crate_release_owned_homes_invalid")),
            }
            let metadata = fs::symlink_metadata(&current)
                .map_err(|_| internal("crate_release_owned_homes_invalid"))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(internal("crate_release_owned_homes_invalid"));
            }
        }
        if path
            .canonicalize()
            .map_err(|_| internal("crate_release_owned_homes_invalid"))?
            != path
        {
            return Err(internal("crate_release_owned_homes_invalid"));
        }
    }
    if [rustup_home, cargo_home]
        .iter()
        .any(|home| source.starts_with(home) || home.starts_with(source))
    {
        return Err(internal("crate_release_toolchain_source_overlap"));
    }
    Ok(())
}
