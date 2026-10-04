#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "impl_mise_git_objects_fixture.rs"]
mod fixture;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use fixture::{add_gitlink, git_dir, independent_commit, install_hook, object_id, repo};
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
    Ok(git_dir(root)?.join("index"))
}

fn run_show(root: &Path, object: &str) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    GitRequest::show(vec![OsString::from(object)])
        .command_in(root)
        .run_cancellable(CAPTURE_LIMIT, Duration::from_secs(10), &cancel)
}

fn run_tree(root: &Path, tree: &str) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    GitRequest::ls_tree(["-r", "-z", tree].into_iter().map(OsString::from).collect())
        .command_in(root)
        .run_cancellable(CAPTURE_LIMIT, Duration::from_secs(10), &cancel)
}

fn run_range(root: &Path, left: &str, right: &str) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    let range = format!("{left}...{right}");
    GitRequest::diff(
        ["--name-only", range.as_str()]
            .into_iter()
            .map(OsString::from)
            .collect(),
    )
    .command_in(root)
    .run_cancellable(CAPTURE_LIMIT, Duration::from_secs(10), &cancel)
}

fn assert_object_refusal(error: MiseError, reason: &str) {
    assert!(
        matches!(
            &error,
            MiseError::InvalidStepInput { field, value }
                if field == "git_object_admission" && value.contains(reason)
        ),
        "unexpected object refusal: {error}"
    );
}

#[test]
fn show_allows_blob_and_refuses_commit_and_tree() -> Result<(), String> {
    let fixture = repo("objects-show", "sha1", 4)?;
    let blob = object_id(&fixture.root, "HEAD:tracked.txt")?;
    let commit = object_id(&fixture.root, "HEAD")?;
    let tree = object_id(&fixture.root, "HEAD^{tree}")?;
    let output = run_show(&fixture.root, &blob).map_err(|error| error.to_string())?;
    assert!(output.success, "blob show failed: {output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"initial\n");
    for object in [commit, tree] {
        let error = run_show(&fixture.root, &object).expect_err("non-blob show must refuse");
        assert_object_refusal(error, "show_object_not_proved_blob");
    }
    Ok(())
}

#[test]
fn show_allows_full_commit_oid_path_to_ordinary_blob() -> Result<(), String> {
    let fixture = repo("objects-show-full-oid", "sha1", 4)?;
    let head = object_id(&fixture.root, "HEAD")?;
    let spec = format!("{head}:tracked.txt");
    let output = run_show(&fixture.root, &spec).map_err(|error| error.to_string())?;
    assert!(output.success, "full commit path show failed: {output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"initial\n");
    Ok(())
}

#[test]
fn revision_tree_with_gitlink_is_refused_before_native_tree_read() -> Result<(), String> {
    let fixture = repo("objects-gitlink", "sha1", 4)?;
    let child = repo("objects-gitlink-child", "sha1", 4)?;
    let tree = add_gitlink(&fixture.root, &child.root, "nested")?;
    let hook = install_hook(&fixture.root)?;
    let index = index_path(&fixture.root)?;
    let before = snapshot(&index)?;
    let error = run_tree(&fixture.root, &tree).expect_err("gitlink tree must refuse");
    assert_object_refusal(error, "revision_tree_unsupported");
    assert_eq!(before, snapshot(&index)?);
    assert!(!hook.exists(), "tree admission reached native Git");
    Ok(())
}

#[test]
fn show_refuses_gitlink_oid_path_before_native_read() -> Result<(), String> {
    let fixture = repo("objects-show-gitlink", "sha1", 4)?;
    let child = repo("objects-show-gitlink-child", "sha1", 4)?;
    let tree_id = add_gitlink(&fixture.root, &child.root, "nested")?;
    let hook = install_hook(&fixture.root)?;
    let index = index_path(&fixture.root)?;
    let before = snapshot(&index)?;
    let spec = format!("{tree_id}:tracked.txt");
    let error = run_show(&fixture.root, &spec).expect_err("gitlink show must refuse");
    assert_object_refusal(error, "revision_tree_unsupported");
    assert_eq!(before, snapshot(&index)?);
    assert!(!hook.exists(), "gitlink show reached native Git");
    Ok(())
}

#[test]
fn unrelated_merge_base_is_refused_before_native_diff() -> Result<(), String> {
    let fixture = repo("objects-mergebase", "sha1", 4)?;
    let left = object_id(&fixture.root, "HEAD")?;
    let right = independent_commit(&fixture.root)?;
    let hook = install_hook(&fixture.root)?;
    let index = index_path(&fixture.root)?;
    let before = snapshot(&index)?;
    let error =
        run_range(&fixture.root, &left, &right).expect_err("missing merge base must refuse");
    assert_object_refusal(error, "merge_base_not_proved");
    assert_eq!(before, snapshot(&index)?);
    assert!(!hook.exists(), "merge-base admission reached native Git");
    Ok(())
}
