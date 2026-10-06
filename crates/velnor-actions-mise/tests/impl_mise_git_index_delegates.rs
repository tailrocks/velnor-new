#![cfg(any(target_os = "linux", target_os = "macos"))]

#[cfg(test)]
#[path = "impl_mise_git_index_fixture.rs"]
mod fixture;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fixture::{Fixture, git_owned, install_hook, repo, touch_identical};
use velnor_actions_mise::{CancelHandle, GitRequest, MiseError, ProcessOutput};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    bytes: Vec<u8>,
    modified: std::time::SystemTime,
}

fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let modified = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .modified()
        .map_err(|error| error.to_string())?;
    Ok(Snapshot {
        bytes: std::fs::read(path).map_err(|error| error.to_string())?,
        modified,
    })
}

fn run_diff(root: &Path) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    GitRequest::diff(vec![OsString::from("--name-only")])
        .command_in(root)
        .run_cancellable(1024 * 1024, Duration::from_secs(10), &cancel)
}

fn run_no_index(root: &Path) -> Result<ProcessOutput, MiseError> {
    let cancel = CancelHandle::new();
    GitRequest::diff(
        [
            "--no-index",
            "--exit-code",
            "--",
            "tracked.txt",
            "other.txt",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
    )
    .command_in(root)
    .run_cancellable(1024 * 1024, Duration::from_secs(10), &cancel)
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn executable_script(path: &Path, marker: &Path, tail: &str) -> Result<(), String> {
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\nprintf marker > {}\n{tail}\n",
            shell_quote(marker)
        ),
    )
    .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn configure_filter(root: &Path, kind: &str) -> Result<PathBuf, String> {
    std::fs::write(root.join(".gitattributes"), b"tracked.txt filter=hostile\n")
        .map_err(|error| error.to_string())?;
    let marker = root.join(format!("{kind}-filter.marker"));
    let script = root.join(format!("{kind}-filter.sh"));
    let tail = if kind == "clean" { "cat" } else { "exit 1" };
    executable_script(&script, &marker, tail)?;
    let key = format!("filter.hostile.{kind}");
    git_owned(
        root,
        vec![
            "config".to_owned(),
            key,
            script
                .to_str()
                .ok_or_else(|| "non-utf8 filter path".to_owned())?
                .to_owned(),
        ],
    )?;
    Ok(marker)
}

fn assert_empty(output: ProcessOutput) -> Result<(), String> {
    if !output.success {
        return Err(format!("diff failed: {output:?}"));
    }
    let text = output
        .stdout_text("git")
        .map_err(|error| error.to_string())?;
    if !text.is_empty() {
        return Err(format!("unexpected diff output: {text:?}"));
    }
    Ok(())
}

#[test]
fn clean_and_process_filters_reject_before_execution() -> Result<(), String> {
    for kind in ["clean", "process"] {
        let fixture = repo("delegate-filter", "sha1", 4)?;
        let post_index_marker = install_hook(&fixture.root)?;
        let index = fixture.root.join(".git/index");
        let before = snapshot(&index)?;
        let filter_marker = configure_filter(&fixture.root, kind)?;
        touch_identical(&fixture.root.join("tracked.txt"))?;
        let error = run_diff(&fixture.root).expect_err("configured filter must reject");
        assert!(
            matches!(error, MiseError::InvalidStepInput { .. }),
            "{error}"
        );
        assert!(!filter_marker.exists(), "filter delegate executed");
        assert!(
            !post_index_marker.exists(),
            "Git ran after filter rejection"
        );
        assert_eq!(before, snapshot(&index)?);
    }
    Ok(())
}

#[test]
fn no_index_filters_reject_before_execution() -> Result<(), String> {
    for kind in ["clean", "process"] {
        let fixture = repo("no-index-filter", "sha1", 4)?;
        let post_index_marker = install_hook(&fixture.root)?;
        let index = fixture.root.join(".git/index");
        let before = snapshot(&index)?;
        let filter_marker = configure_filter(&fixture.root, kind)?;
        std::fs::write(fixture.root.join("tracked.txt"), "left\n")
            .map_err(|error| error.to_string())?;
        std::fs::write(fixture.root.join("other.txt"), "right\n")
            .map_err(|error| error.to_string())?;
        let error = run_no_index(&fixture.root).expect_err("no-index filter must reject");
        assert!(
            matches!(error, MiseError::InvalidStepInput { .. }),
            "{error}"
        );
        assert!(!filter_marker.exists(), "no-index filter delegate executed");
        assert!(
            !post_index_marker.exists(),
            "Git ran after no-index rejection"
        );
        assert_eq!(before, snapshot(&index)?);
    }
    Ok(())
}

#[test]
fn checksum_tampering_rejects_private_index_for_both_oid_widths() -> Result<(), String> {
    for object_format in ["sha1", "sha256"] {
        let fixture = repo("checksum-tamper", object_format, 4)?;
        let post_index_marker = install_hook(&fixture.root)?;
        let filter_marker = configure_filter(&fixture.root, "clean")?;
        let index = fixture.root.join(".git/index");
        let mut bytes = std::fs::read(&index).map_err(|error| error.to_string())?;
        let last = bytes
            .last_mut()
            .ok_or_else(|| "index unexpectedly empty".to_owned())?;
        *last ^= 1;
        std::fs::write(&index, bytes).map_err(|error| error.to_string())?;
        touch_identical(&fixture.root.join("tracked.txt"))?;
        let tampered = snapshot(&index)?;
        let error = run_diff(&fixture.root).expect_err("checksum tampering must reject");
        assert!(
            error.to_string().contains("checksum"),
            "unexpected error: {error}"
        );
        assert_eq!(tampered, snapshot(&index)?);
        assert!(
            !post_index_marker.exists(),
            "Git ran after checksum rejection"
        );
        assert!(!filter_marker.exists(), "filter delegate executed");
    }
    Ok(())
}

fn assert_object_format_mismatch(error: MiseError) {
    assert!(
        matches!(
            &error,
            MiseError::InvalidStepInput { field, value }
                if field == "git_object_format" && value.contains("mismatch")
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn object_format_mismatch_rejects_before_native_index_read() -> Result<(), String> {
    let sha1 = repo("object-format-sha1", "sha1", 4)?;
    let sha256 = repo("object-format-sha256", "sha256", 4)?;
    let sha1_marker = install_hook(&sha1.root)?;
    let sha256_marker = install_hook(&sha256.root)?;
    let sha1_index = sha1.root.join(".git/index");
    let sha256_index = sha256.root.join(".git/index");
    let sha1_bytes = std::fs::read(&sha1_index).map_err(|error| error.to_string())?;
    let sha256_bytes = std::fs::read(&sha256_index).map_err(|error| error.to_string())?;

    std::fs::write(&sha256_index, &sha1_bytes).map_err(|error| error.to_string())?;
    let sha256_before = snapshot(&sha256_index)?;
    let error = run_diff(&sha256.root).expect_err("SHA-1 index in SHA-256 repo must reject");
    assert_object_format_mismatch(error);
    assert_eq!(sha256_before, snapshot(&sha256_index)?);
    assert!(
        !sha256_marker.exists(),
        "Git ran after object-format rejection"
    );

    std::fs::write(&sha1_index, &sha256_bytes).map_err(|error| error.to_string())?;
    let sha1_before = snapshot(&sha1_index)?;
    let error = run_diff(&sha1.root).expect_err("SHA-256 index in SHA-1 repo must reject");
    assert_object_format_mismatch(error);
    assert_eq!(sha1_before, snapshot(&sha1_index)?);
    assert!(
        !sha1_marker.exists(),
        "Git ran after object-format rejection"
    );
    Ok(())
}

#[test]
fn configured_fsmonitor_and_post_index_hooks_stay_disabled() -> Result<(), String> {
    let fixture = repo("fsmonitor-hook", "sha1", 4)?;
    let post_index_marker = install_hook(&fixture.root)?;
    let fsmonitor_marker = fixture.root.join("fsmonitor.marker");
    let fsmonitor_script = fixture.root.join("fsmonitor.sh");
    executable_script(&fsmonitor_script, &fsmonitor_marker, "exit 0")?;
    git_owned(
        &fixture.root,
        vec![
            "config".to_owned(),
            "core.fsmonitor".to_owned(),
            fsmonitor_script
                .to_str()
                .ok_or_else(|| "non-utf8 fsmonitor path".to_owned())?
                .to_owned(),
        ],
    )?;
    let index = fixture.root.join(".git/index");
    let before = snapshot(&index)?;
    touch_identical(&fixture.root.join("tracked.txt"))?;
    assert_empty(run_diff(&fixture.root).map_err(|error| error.to_string())?)?;
    assert_eq!(before, snapshot(&index)?);
    assert!(
        !fsmonitor_marker.exists(),
        "configured fsmonitor hook executed"
    );
    assert!(!post_index_marker.exists(), "post-index hook executed");
    Ok(())
}
