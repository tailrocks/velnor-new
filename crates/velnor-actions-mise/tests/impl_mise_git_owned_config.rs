#![cfg(any(target_os = "linux", target_os = "macos"))]

#[cfg(test)]
#[path = "impl_mise_git_owned_config_fixture.rs"]
mod fixture;

use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, SystemTime};

use fixture::{
    Fixture, add_native_values, configure_filter, configure_metadata, configure_trace_target,
    git_dir, git_fixture, install_hook, repo, touch_identical,
};
use velnor_actions_mise::{CancelHandle, GitRequest, MiseError, ProcessOutput};

const CAPTURE_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    bytes: Vec<u8>,
    modified: SystemTime,
    links: u64,
}

fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    Ok(Snapshot {
        bytes: std::fs::read(path).map_err(|error| error.to_string())?,
        modified: metadata.modified().map_err(|error| error.to_string())?,
        links: metadata.nlink(),
    })
}

fn index_path(root: &Path) -> Result<PathBuf, String> {
    let metadata = git_dir(root)?;
    Ok(metadata.join("index"))
}

fn run_diff(root: &Path) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    GitRequest::diff(vec![OsString::from("--name-only")])
        .command_in(root)
        .run_cancellable(CAPTURE_LIMIT, Duration::from_secs(10), &cancel)
}

fn run_no_index(root: &Path) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    GitRequest::diff(
        ["--no-index", "--name-only", "--", "left.txt", "right.txt"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    )
    .command_in(root)
    .run_cancellable(CAPTURE_LIMIT, Duration::from_secs(10), &cancel)
}

fn raw_name_only(root: &Path) -> Result<Output, String> {
    git_fixture::command(root)
        .map_err(|error| error.to_string())?
        .args(["diff", "--name-only"])
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())
}

fn assert_filter_refusal(error: MiseError) {
    let text = error.to_string();
    assert!(
        matches!(&error, MiseError::InvalidStepInput { .. }),
        "filter admission must be typed: {text}"
    );
    assert!(text.to_ascii_lowercase().contains("filter"), "{text}");
}

#[test]
fn included_clean_and_process_filters_reject_before_delegate_execution() -> Result<(), String> {
    for layer in ["include", "nested-include"] {
        for operation in ["clean", "process"] {
            let fixture = repo(&format!("{layer}-{operation}"), "sha1", 4)?;
            let hook_marker = install_hook(&fixture.root)?;
            let index = index_path(&fixture.root)?;
            let before = snapshot(&index)?;
            let filter_marker = configure_filter(&fixture.root, layer, operation)?;
            touch_identical(&fixture.root.join("tracked.txt"))?;
            let error = run_diff(&fixture.root).expect_err("configured filter must refuse");
            assert_filter_refusal(error);
            assert_eq!(before, snapshot(&index)?);
            assert!(!filter_marker.exists(), "filter delegate executed");
            assert!(!hook_marker.exists(), "post-index hook executed");
        }
    }
    Ok(())
}

#[test]
fn native_config_values_round_trip_without_changing_controlled_output() -> Result<(), String> {
    let fixture = repo("native-config-values", "sha1", 4)?;
    add_native_values(&fixture.root)?;
    std::fs::write(fixture.root.join("tracked.txt"), "changed\n")
        .map_err(|error| error.to_string())?;
    let raw = raw_name_only(&fixture.root)?;
    let private = run_diff(&fixture.root).map_err(|error| error.to_string())?;
    assert_eq!(private.success, raw.status.success());
    assert_eq!(private.stdout, raw.stdout);
    Ok(())
}

#[test]
fn trace2_config_target_is_suppressed_during_source_config_capture() -> Result<(), String> {
    let fixture = repo("trace2-config", "sha1", 4)?;
    let hook_marker = install_hook(&fixture.root)?;
    let trace_marker = configure_trace_target(&fixture.root)?;
    let index = index_path(&fixture.root)?;
    let before = snapshot(&index)?;
    touch_identical(&fixture.root.join("tracked.txt"))?;
    let output = run_diff(&fixture.root).map_err(|error| error.to_string())?;
    assert!(output.success, "identical diff failed: {output:?}");
    assert_eq!(before, snapshot(&index)?);
    assert!(
        !trace_marker.exists(),
        "source config query wrote trace2 output"
    );
    assert!(!hook_marker.exists(), "post-index hook executed");
    Ok(())
}

#[test]
fn no_index_outside_repository_does_not_require_source_config() -> Result<(), String> {
    let fixture = Fixture::new("owned-config-no-index")?;
    std::fs::write(fixture.root.join("left.txt"), "same\n").map_err(|error| error.to_string())?;
    std::fs::write(fixture.root.join("right.txt"), "same\n").map_err(|error| error.to_string())?;
    let output = run_no_index(&fixture.root).map_err(|error| error.to_string())?;
    assert!(
        output.success,
        "no-index outside repository failed: {output:?}"
    );
    Ok(())
}

#[test]
fn unsupported_repository_metadata_is_refused_before_native_diff() -> Result<(), String> {
    for kind in [
        "unknown",
        "promisor",
        "alternates",
        "shallow",
        "replace",
        "gitlinks",
    ] {
        let fixture = repo(&format!("metadata-{kind}"), "sha1", 4)?;
        let hook_marker = install_hook(&fixture.root)?;
        let index = index_path(&fixture.root)?;
        let before = snapshot(&index)?;
        configure_metadata(&fixture.root, kind)?;
        let error = run_diff(&fixture.root).expect_err("unsupported metadata must refuse");
        assert!(!error.to_string().is_empty(), "untyped metadata refusal");
        assert_eq!(before, snapshot(&index)?);
        assert!(!hook_marker.exists(), "metadata refusal reached native Git");
    }
    Ok(())
}

#[test]
fn discovery_git_rejects_explicit_path_override() {
    let command = GitRequest::diff(vec![OsString::from("--name-only")]).command();
    let error = command
        .with_env(&[(OsString::from("PATH"), OsString::from("/hostile"))])
        .expect_err("Discovery Git must bind its own executable");
    assert!(matches!(error, MiseError::InvalidStepInput { field, .. } if field == "PATH"));
}

#[test]
fn discovery_git_rejects_and_scrubs_loader_overrides() {
    let keys = [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "LD_DEBUG_OUTPUT",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "DYLD_PRINT_TO_FILE",
        "GLIBC_TUNABLES",
        "GCONV_PATH",
    ];
    let command = GitRequest::diff(vec![OsString::from("--name-only")]).command();
    let parent = keys
        .iter()
        .map(|key| (OsString::from(*key), OsString::from("hostile")))
        .chain([(OsString::from("PATH"), OsString::from("/ambient"))])
        .collect::<Vec<_>>();
    for key in keys {
        let error = command
            .clone()
            .with_env(&[(key.into(), "hostile".into())])
            .expect_err("caller loader override refused");
        assert!(matches!(error, MiseError::InvalidStepInput { field, .. } if field == key));
    }
    let child = command.spawn_env(&parent);
    assert!(
        !child
            .iter()
            .any(|(key, _)| keys.iter().any(|name| key == *name))
    );
    assert!(
        child
            .iter()
            .any(|(key, value)| key == "PATH" && value == "/ambient")
    );
}
