//! Credential-free capture of the public default branch of a Homebrew tap.

use crate::{CheckDeadline, command::IsolatedCommand, error::MiseError};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[path = "git_tap_capture/homebrew_pinned_source.rs"]
pub mod homebrew_pinned_source;

pub use homebrew_pinned_source::{PinnedHomebrewSource, materialize_homebrew_pinned_source};

const OUTPUT_LIMIT: usize = 1024 * 1024;

/// A fresh detached checkout of the exact observed `main` commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedTapMain {
    /// Full lowercase object ID returned by `ls-remote` and rechecked after fetch.
    pub commit_sha: String,
    /// Isolated repository materialized at that commit.
    pub checkout: PathBuf,
}

/// One regular Git tree entry at the captured tap base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapTreeEntry {
    /// Repository-relative UTF-8 path.
    pub path: String,
    /// Git mode, restricted to regular files by this capture contract.
    pub mode: u32,
    /// Full lowercase Git blob object ID.
    pub blob_sha: String,
}

/// Capture public tap `main` and materialize only that observed commit.
///
/// This uses a fixed HTTPS remote derived from a validated owner/repository
/// slug. It never consults ambient Git configuration or credentials, and it
/// disables helpers, hooks, submodules, and global/system configuration.
///
/// # Errors
/// The caller must create an empty absolute destination directory before
/// calling this function. Rejects malformed slugs, remote races, dirty
/// materialization, Git failures, or an expired deadline.
pub fn capture_tap_main(
    repository: &str,
    destination: &Path,
    deadline: CheckDeadline,
) -> Result<CapturedTapMain, MiseError> {
    validate_repository(repository)?;
    if !destination.is_absolute() {
        return Err(invalid("tap_checkout_destination"));
    }
    let sha = capture_remote_sha(repository, deadline)?;
    materialize(repository, &sha, destination, deadline)?;
    Ok(CapturedTapMain {
        commit_sha: sha,
        checkout: destination.to_path_buf(),
    })
}

/// Read the exact regular-file tree of the previously captured commit.
///
/// # Errors
/// Rejects nonregular entries, malformed tree records, or Git failures.
pub fn read_tap_tree(
    checkout: &Path,
    commit_sha: &str,
    deadline: CheckDeadline,
) -> Result<Vec<TapTreeEntry>, MiseError> {
    if !is_full_sha(commit_sha) {
        return Err(invalid("tap_tree_sha"));
    }
    let output = run_git(
        Some(checkout),
        ["ls-tree", "-r", "-z", "--full-tree", commit_sha],
        deadline,
    )?;
    output.require_success("git")?;
    parse_tree(&output.stdout)
}

fn parse_tree(bytes: &[u8]) -> Result<Vec<TapTreeEntry>, MiseError> {
    let mut entries = Vec::new();
    for record in bytes.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| invalid("tap_tree_record"))?;
        let (metadata, remainder) = record.split_at(tab);
        let path = &remainder[1..];
        let mut fields = metadata.split(|byte| *byte == b' ');
        let mode = fields
            .next()
            .and_then(|value| std::str::from_utf8(value).ok())
            .and_then(|value| u32::from_str_radix(value, 8).ok())
            .ok_or_else(|| invalid("tap_tree_mode"))?;
        let object_type = fields
            .next()
            .and_then(|value| std::str::from_utf8(value).ok())
            .ok_or_else(|| invalid("tap_tree_record"))?;
        let sha = fields
            .next()
            .and_then(|value| std::str::from_utf8(value).ok())
            .filter(|value| is_full_sha(value))
            .ok_or_else(|| invalid("tap_tree_blob_sha"))?;
        if fields.next().is_some() {
            return Err(invalid("tap_tree_record"));
        }
        let path = std::str::from_utf8(path)
            .map_err(|_| invalid("tap_tree_non_utf8_path"))?
            .to_owned();
        if entries
            .iter()
            .any(|entry: &TapTreeEntry| entry.path == path)
        {
            return Err(invalid("tap_tree_path"));
        }
        entries.push(validate_tap_regular_leaf(&path, mode, object_type, sha)?);
    }
    if entries.is_empty() {
        return Err(invalid("tap_tree_empty"));
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

/// Admit one regular leaf from a captured Git tree or a recursive API tree.
///
/// # Errors
/// Rejects unsafe paths, nonregular modes/types, or malformed blob identities.
pub fn validate_tap_regular_leaf(
    path: &str,
    mode: u32,
    object_type: &str,
    blob_sha: &str,
) -> Result<TapTreeEntry, MiseError> {
    if !matches!(mode, 0o100_644 | 0o100_755) || object_type != "blob" {
        return Err(invalid("tap_tree_nonregular_entry"));
    }
    if !safe_tree_path(path) {
        return Err(invalid("tap_tree_path"));
    }
    if !is_full_sha(blob_sha) {
        return Err(invalid("tap_tree_blob_sha"));
    }
    Ok(TapTreeEntry {
        path: path.to_owned(),
        mode,
        blob_sha: blob_sha.to_owned(),
    })
}

fn safe_tree_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn capture_remote_sha(repository: &str, deadline: CheckDeadline) -> Result<String, MiseError> {
    let url = format!("https://github.com/{repository}.git");
    let output = run_git(
        None,
        [
            "ls-remote",
            "--exit-code",
            "--refs",
            &url,
            "refs/heads/main",
        ],
        deadline,
    )?;
    output.require_success("git")?;
    let text = output.stdout_text("git")?;
    let mut lines = text.lines();
    let line = lines.next().ok_or_else(|| invalid("tap_main_missing"))?;
    let (sha, reference) = line
        .split_once('\t')
        .ok_or_else(|| invalid("tap_main_response"))?;
    if lines.next().is_some() || reference != "refs/heads/main" || !is_full_sha(sha) {
        return Err(invalid("tap_main_response"));
    }
    Ok(sha.to_owned())
}

fn materialize(
    repository: &str,
    expected_sha: &str,
    destination: &Path,
    deadline: CheckDeadline,
) -> Result<(), MiseError> {
    run_git(
        Some(destination),
        ["init", "--quiet", "--template=/dev/null", "."],
        deadline,
    )?
    .require_success("git")?;
    let url = format!("https://github.com/{repository}.git");
    run_git(
        Some(destination),
        [
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            &url,
            "+refs/heads/main:refs/remotes/velnor/main",
        ],
        deadline,
    )?
    .require_success("git")?;
    let fetched = run_git(
        Some(destination),
        ["rev-parse", "--verify", "refs/remotes/velnor/main^{commit}"],
        deadline,
    )?
    .require_success("git")?
    .stdout_text("git")?
    .trim()
    .to_owned();
    if fetched != expected_sha {
        return Err(invalid("tap_main_advanced_during_capture"));
    }
    run_git(
        Some(destination),
        ["checkout", "--detach", "--force", expected_sha],
        deadline,
    )?
    .require_success("git")?;
    let head = run_git(
        Some(destination),
        ["rev-parse", "--verify", "HEAD^{commit}"],
        deadline,
    )?
    .require_success("git")?
    .stdout_text("git")?
    .trim()
    .to_owned();
    let status = run_git(
        Some(destination),
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        deadline,
    )?
    .require_success("git")?
    .stdout
    .clone();
    if head != expected_sha || !status.is_empty() {
        return Err(invalid("tap_checkout_not_clean_at_captured_main"));
    }
    Ok(())
}

fn run_git<const N: usize>(
    cwd: Option<&Path>,
    words: [&str; N],
    deadline: CheckDeadline,
) -> Result<crate::ProcessOutput, MiseError> {
    let mut git_args = vec![
        OsString::from("-c"),
        OsString::from("credential.helper="),
        OsString::from("-c"),
        OsString::from("core.hooksPath=/dev/null"),
        OsString::from("-c"),
        OsString::from("protocol.file.allow=never"),
        OsString::from("-c"),
        OsString::from("submodule.recurse=false"),
    ];
    git_args.extend(words.into_iter().map(OsString::from));
    let mut command = IsolatedCommand::direct("git", git_args);
    if let Some(cwd) = cwd {
        command = command.with_cwd(cwd.to_path_buf());
    }
    command = command.with_env(&[
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        ("GIT_ALLOW_PROTOCOL".into(), "https".into()),
    ])?;
    command.run_until(OUTPUT_LIMIT, deadline)
}

fn validate_repository(repository: &str) -> Result<(), MiseError> {
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(invalid("tap_repository_slug"));
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && !part.starts_with('.')
            && !part.ends_with('.')
    };
    if !valid(owner) || !valid(name) || repository.matches('/').count() != 1 {
        return Err(invalid("tap_repository_slug"));
    }
    Ok(())
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn invalid(problem: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "tap_git_capture".to_owned(),
        value: problem.to_owned(),
    }
}

#[cfg(test)]
#[path = "git_tap_capture_tests.rs"]
mod tests;
