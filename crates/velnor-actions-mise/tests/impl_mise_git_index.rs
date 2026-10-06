#![cfg(any(target_os = "linux", target_os = "macos"))]

#[cfg(test)]
#[path = "impl_mise_git_index_fixture.rs"]
mod fixture;

#[cfg(test)]
#[path = "impl_mise_git_index_selectors.rs"]
mod selectors;

use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use fixture::{Fixture, git, git_dir, git_output, git_owned, install_hook, repo, touch_identical};
use velnor_actions_mise::{CancelHandle, GitRequest, MiseError, ProcessOutput};

const CAPTURE_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    path: PathBuf,
    bytes: Vec<u8>,
    modified: SystemTime,
}

fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let modified = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .modified()
        .map_err(|error| error.to_string())?;
    Ok(Snapshot {
        path: path.to_owned(),
        bytes: std::fs::read(path).map_err(|error| error.to_string())?,
        modified,
    })
}

fn index_path(root: &Path) -> Result<PathBuf, String> {
    let output = git_output(root, &["rev-parse", "--git-path", "index"])?;
    let path = PathBuf::from(output.trim());
    if path.is_absolute() {
        Ok(path)
    } else {
        root.join(path)
            .canonicalize()
            .map_err(|error| error.to_string())
    }
}

fn assert_index_version(path: &Path, expected: u8) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let header = bytes
        .get(4..8)
        .ok_or_else(|| "index header is truncated".to_owned())?;
    let actual = u32::from_be_bytes(
        header
            .try_into()
            .map_err(|_| "index version header is malformed".to_owned())?,
    );
    assert_eq!(actual, u32::from(expected));
    Ok(())
}

fn shared_snapshots(root: &Path) -> Result<Vec<Snapshot>, String> {
    let mut snapshots = Vec::new();
    for entry in std::fs::read_dir(git_dir(root)?).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("sharedindex.")
        {
            snapshots.push(snapshot(&entry.path())?);
        }
    }
    snapshots.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(snapshots)
}

fn diff_request() -> GitRequest {
    GitRequest::diff(vec![OsString::from("--name-only")])
}

fn run_diff(root: &Path) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    diff_request()
        .command_in(root)
        .run_cancellable(CAPTURE_LIMIT, Duration::from_secs(10), &cancel)
}

fn no_index_request(left: &str, right: &str) -> GitRequest {
    GitRequest::diff(
        ["--no-index", "--name-only", "--", left, right]
            .into_iter()
            .map(OsString::from)
            .collect(),
    )
}

fn run_no_index(root: &Path, left: &str, right: &str) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    no_index_request(left, right)
        .command_in(root)
        .run_cancellable(CAPTURE_LIMIT, Duration::from_secs(10), &cancel)
}

fn assert_empty_diff(output: ProcessOutput) -> Result<(), String> {
    if !output.success {
        return Err(format!("identical diff failed: {output:?}"));
    }
    let text = output
        .stdout_text("git")
        .map_err(|error| error.to_string())?;
    if !text.is_empty() {
        return Err(format!("identical diff listed paths: {text:?}"));
    }
    Ok(())
}

fn assert_changed_path(output: ProcessOutput) -> Result<(), String> {
    if !output.success {
        return Err(format!("changed diff failed: {output:?}"));
    }
    let text = output
        .stdout_text("git")
        .map_err(|error| error.to_string())?;
    if text.trim() != "tracked.txt" {
        return Err(format!("changed diff listed {text:?}"));
    }
    Ok(())
}

#[test]
fn private_diff_supports_sha1_sha256_and_index_versions() -> Result<(), String> {
    for object_format in ["sha1", "sha256"] {
        for version in 2..=4 {
            let fixture = repo("matrix", object_format, version)?;
            let marker = install_hook(&fixture.root)?;
            let index = index_path(&fixture.root)?;
            assert_index_version(&index, version)?;
            let before = snapshot(&index)?;
            touch_identical(&fixture.root.join("tracked.txt"))?;
            assert_empty_diff(run_diff(&fixture.root).map_err(|error| error.to_string())?)?;
            assert_eq!(before, snapshot(&index)?);
            assert!(!marker.exists(), "private diff must not run the hook");

            std::fs::write(fixture.root.join("tracked.txt"), "changed\n")
                .map_err(|error| error.to_string())?;
            let before_changed = snapshot(&index)?;
            assert_changed_path(run_diff(&fixture.root).map_err(|error| error.to_string())?)?;
            assert_eq!(before_changed, snapshot(&index)?);
            assert!(!marker.exists(), "private diff must not run the hook");
        }
    }
    Ok(())
}

#[test]
fn source_split_config_stays_full_and_private_diff_does_not_create_sharedindex()
-> Result<(), String> {
    let fixture = repo("split-config", "sha1", 4)?;
    let marker = install_hook(&fixture.root)?;
    git(&fixture.root, &["config", "core.splitIndex", "true"])?;
    assert!(shared_snapshots(&fixture.root)?.is_empty());
    let index = index_path(&fixture.root)?;
    let before = snapshot(&index)?;
    touch_identical(&fixture.root.join("tracked.txt"))?;
    assert_empty_diff(run_diff(&fixture.root).map_err(|error| error.to_string())?)?;
    assert_eq!(before, snapshot(&index)?);
    assert!(shared_snapshots(&fixture.root)?.is_empty());
    assert!(!marker.exists(), "private diff must not run the hook");
    Ok(())
}

#[test]
fn split_index_is_rejected_before_git_and_preserves_source_files() -> Result<(), String> {
    let fixture = repo("split-reject", "sha1", 3)?;
    git(&fixture.root, &["update-index", "--split-index"])?;
    let marker = install_hook(&fixture.root)?;
    let index = index_path(&fixture.root)?;
    let index_before = snapshot(&index)?;
    let shared_before = shared_snapshots(&fixture.root)?;
    assert!(!shared_before.is_empty(), "split fixture needs sharedindex");
    let error = run_diff(&fixture.root).expect_err("split index must reject before Git");
    assert!(
        error.to_string().contains("split"),
        "unexpected error: {error}"
    );
    assert_eq!(index_before, snapshot(&index)?);
    assert_eq!(shared_before, shared_snapshots(&fixture.root)?);
    assert!(!marker.exists(), "split rejection must happen before Git");

    std::fs::write(fixture.root.join("left.txt"), "same\n").map_err(|error| error.to_string())?;
    std::fs::write(fixture.root.join("right.txt"), "same\n").map_err(|error| error.to_string())?;
    let output =
        run_no_index(&fixture.root, "left.txt", "right.txt").map_err(|error| error.to_string())?;
    assert!(
        output.success,
        "no-index must bypass split index: {output:?}"
    );
    Ok(())
}

#[test]
fn malformed_index_is_rejected_before_git() -> Result<(), String> {
    let fixture = repo("malformed", "sha1", 2)?;
    let marker = install_hook(&fixture.root)?;
    let index = index_path(&fixture.root)?;
    std::fs::write(&index, b"malformed index").map_err(|error| error.to_string())?;
    let error = run_diff(&fixture.root).expect_err("malformed index must reject");
    assert!(!error.to_string().is_empty());
    assert_eq!(
        std::fs::read(&index).map_err(|error| error.to_string())?,
        b"malformed index"
    );
    assert!(!marker.exists(), "rejection must happen before Git");
    Ok(())
}

#[test]
fn symlink_index_is_rejected_before_git() -> Result<(), String> {
    let fixture = repo("symlink-index", "sha1", 2)?;
    let index = index_path(&fixture.root)?;
    let target = fixture.root.join("index-target");
    std::fs::copy(&index, &target).map_err(|error| error.to_string())?;
    std::fs::remove_file(&index).map_err(|error| error.to_string())?;
    std::os::unix::fs::symlink(&target, &index).map_err(|error| error.to_string())?;
    let error = run_diff(&fixture.root).expect_err("symlink index must reject");
    assert!(!error.to_string().is_empty());
    assert!(
        std::fs::symlink_metadata(&index)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
    );
    Ok(())
}

#[test]
fn hardlink_index_is_rejected_before_git() -> Result<(), String> {
    let fixture = repo("hardlink-index", "sha1", 2)?;
    let index = index_path(&fixture.root)?;
    let alias = fixture.root.join("index-alias");
    std::fs::copy(&index, &alias).map_err(|error| error.to_string())?;
    std::fs::remove_file(&index).map_err(|error| error.to_string())?;
    std::fs::hard_link(&alias, &index).map_err(|error| error.to_string())?;
    let error = run_diff(&fixture.root).expect_err("hardlink index must reject");
    assert!(!error.to_string().is_empty());
    assert!(
        std::fs::metadata(&index)
            .map_err(|error| error.to_string())?
            .nlink()
            > 1
    );
    Ok(())
}

#[test]
fn missing_index_in_unborn_repo_is_supported() -> Result<(), String> {
    let fixture = fixture::unborn("missing-index")?;
    let index = fixture.root.join(".git/index");
    assert!(!index.exists());
    assert_empty_diff(run_diff(&fixture.root).map_err(|error| error.to_string())?)?;
    assert!(!index.exists(), "unborn read must not create source index");
    Ok(())
}

#[test]
fn no_index_works_outside_a_repository() -> Result<(), String> {
    let fixture = Fixture::new("no-index-outside")?;
    std::fs::write(fixture.root.join("left.txt"), "same\n").map_err(|error| error.to_string())?;
    std::fs::write(fixture.root.join("right.txt"), "same\n").map_err(|error| error.to_string())?;
    let output =
        run_no_index(&fixture.root, "left.txt", "right.txt").map_err(|error| error.to_string())?;
    assert!(output.success, "no-index outside repo failed: {output:?}");
    Ok(())
}

#[test]
fn linked_worktree_diff_uses_its_private_index() -> Result<(), String> {
    let fixture = repo("linked-worktree", "sha1", 4)?;
    let child = fixture.root.join("child");
    git_owned(
        &fixture.root,
        vec![
            "worktree".to_owned(),
            "add".to_owned(),
            child.display().to_string(),
            "-b".to_owned(),
            "linked".to_owned(),
        ],
    )?;
    let marker = install_hook(&fixture.root)?;
    let index = index_path(&child)?;
    let before = snapshot(&index)?;
    touch_identical(&child.join("tracked.txt"))?;
    assert_empty_diff(run_diff(&child).map_err(|error| error.to_string())?)?;
    assert_eq!(before, snapshot(&index)?);
    assert!(
        !marker.exists(),
        "linked private diff must not run the hook"
    );
    Ok(())
}
