use std::fs;
use std::os::unix::fs::symlink;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use bollard::container::LogOutput;
use bollard::exec::StartExecResults;
use bollard::models::ExecInspectResponse;
use bytes::Bytes;
use futures_util::stream;

use crate::error::HostError;

#[path = "collect_tests/orb_exec.rs"]
mod orb_exec;

use super::super::MAX_ARCHIVE_BYTES;
use super::{
    ARCHIVE_SCRIPT, EXEC_USER, collect_exec_output, create_exec_options, empty_root_archive,
    start_exec_options, validate_exec_exit, validate_exec_id,
};

#[test]
fn exec_request_uses_fixed_system_path_and_is_unprivileged() {
    let options = create_exec_options();
    assert_eq!(options.user.as_deref(), Some(EXEC_USER));
    assert_eq!(options.attach_stdin, Some(false));
    assert_eq!(options.attach_stdout, Some(true));
    assert_eq!(options.attach_stderr, Some(false));
    assert_eq!(options.tty, Some(false));
    assert_eq!(options.privileged, Some(false));
    assert_eq!(options.env, Some(vec!["PATH=/usr/bin:/bin".to_owned()]));
    assert_eq!(options.working_dir.as_deref(), Some("/home/runner"));
    assert_eq!(
        options.cmd,
        Some(vec![
            "/bin/bash".to_owned(),
            "-c".to_owned(),
            ARCHIVE_SCRIPT.to_owned()
        ])
    );
    assert!(!start_exec_options().detach);
    assert!(!start_exec_options().tty);
    assert!(ARCHIVE_SCRIPT.contains("diag=/home/runner/_diag"));
    assert!(ARCHIVE_SCRIPT.contains("[ ! -L \"$diag\" ]"));
    assert!(ARCHIVE_SCRIPT.contains("[ ! -L \"$file\" ]"));
    assert!(ARCHIVE_SCRIPT.contains("--no-recursion"));
    assert!(ARCHIVE_SCRIPT.contains("/usr/bin/tar --format=ustar"));
    assert!(!ARCHIVE_SCRIPT.contains("\ntar --format=ustar"));
    assert!(!ARCHIVE_SCRIPT.contains("pages"));
    assert!(!ARCHIVE_SCRIPT.contains("blocks"));
    assert!(!ARCHIVE_SCRIPT.contains("eval"));
}

#[test]
fn diag_root_guard_distinguishes_absence_from_unsafe_paths() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!(
        "velnor-runner-log-root-guard-{}",
        std::process::id()
    ));
    drop(fs::remove_dir_all(&root));
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    let missing = root.join("missing");
    let file = root.join("regular-file");
    fs::write(&file, b"not a directory").map_err(|error| error.to_string())?;
    let target = root.join("target");
    fs::create_dir(&target).map_err(|error| error.to_string())?;
    let link = root.join("symlink");
    symlink(&target, &link).map_err(|error| error.to_string())?;
    let broken_link = root.join("broken-symlink");
    symlink(root.join("absent-target"), &broken_link).map_err(|error| error.to_string())?;
    let directory = root.join("directory");
    fs::create_dir(&directory).map_err(|error| error.to_string())?;

    if run_root_guard(&missing)? != Some(3)
        || run_root_guard(&file)? != Some(24)
        || run_root_guard(&link)? != Some(24)
        || run_root_guard(&broken_link)? != Some(24)
        || run_root_guard(&directory)? != Some(0)
    {
        return Err("the _diag root guard confused absence with an unsafe path".to_owned());
    }
    fs::remove_dir_all(root).map_err(|error| error.to_string())?;
    Ok(())
}

fn run_root_guard(path: &std::path::Path) -> Result<Option<i32>, String> {
    let prefix = ARCHIVE_SCRIPT
        .split("command -v find")
        .next()
        .ok_or_else(|| "archive script has no tool preflight".to_owned())?
        .replace("diag=/home/runner/_diag", "diag=\"$1\"");
    let status = std::process::Command::new("/bin/bash")
        .arg("-c")
        .arg(prefix)
        .arg("bash")
        .arg(path)
        .status()
        .map_err(|error| error.to_string())?;
    Ok(status.code())
}

#[test]
fn actual_named_runner_home_archive_is_accepted_without_persisting_raw_text() -> Result<(), String>
{
    let archive = include_bytes!("collect_tests/orb-volume-archive.tar");
    let projection = super::super::projection::sanitize_archive(archive)
        .map_err(|error| format!("{error:?}"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&projection).map_err(|error| error.to_string())?;
    if value["runner_log_present"] != true
        || value["worker_log_present"] != true
        || value["complete"] != true
        || value["unrecognized_lines"] != 2
    {
        return Err("actual RunnerHome archive produced the wrong safe projection".to_owned());
    }
    if projection
        .windows(b"synthetic runner diagnostic".len())
        .any(|window| window == b"synthetic runner diagnostic")
        || projection
            .windows(b"synthetic worker diagnostic".len())
            .any(|window| window == b"synthetic worker diagnostic")
    {
        return Err("actual RunnerHome archive message text reached the projection".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn attached_output_is_bounded_and_only_stdout_is_accepted() {
    let bytes = Bytes::from_static(b"archive");
    let output = attached(vec![Ok(LogOutput::StdOut {
        message: bytes.clone(),
    })]);
    let captured = collect_exec_output(output, Instant::now(), &AtomicBool::new(false))
        .await
        .expect("stdout capture succeeds");
    assert_eq!(captured, b"archive");

    let stderr = attached(vec![Ok(LogOutput::StdErr {
        message: bytes.clone(),
    })]);
    assert_eq!(
        collect_exec_output(stderr, Instant::now(), &AtomicBool::new(false)).await,
        Err(HostError::Ownership)
    );

    let failed_stream = attached(vec![Err(bollard::errors::Error::RequestTimeoutError)]);
    assert_eq!(
        collect_exec_output(failed_stream, Instant::now(), &AtomicBool::new(false)).await,
        Err(HostError::Docker)
    );

    let oversized = attached(vec![Ok(LogOutput::StdOut {
        message: Bytes::from(vec![0; MAX_ARCHIVE_BYTES + 1]),
    })]);
    assert_eq!(
        collect_exec_output(oversized, Instant::now(), &AtomicBool::new(false)).await,
        Err(HostError::Ownership)
    );
}

#[tokio::test]
async fn cancelled_or_expired_capture_fails_before_reading_output() {
    let output = attached(vec![Ok(LogOutput::StdOut {
        message: Bytes::from_static(b"archive"),
    })]);
    assert_eq!(
        collect_exec_output(output, Instant::now(), &AtomicBool::new(true)).await,
        Err(HostError::DockerTimeout)
    );

    let output = attached(Vec::new());
    assert_eq!(
        collect_exec_output(
            output,
            Instant::now()
                .checked_sub(Duration::from_secs(16))
                .expect("test timestamp is representable"),
            &AtomicBool::new(false)
        )
        .await,
        Err(HostError::DockerTimeout)
    );
}

#[test]
fn missing_diag_is_an_empty_valid_archive_but_exec_failures_are_rejected() {
    let empty = empty_root_archive().expect("synthetic absent-diag archive");
    let exec_id = "b".repeat(64);
    let result = ExecInspectResponse {
        id: Some(exec_id.clone()),
        running: Some(false),
        exit_code: Some(3),
        ..Default::default()
    };
    let archive = validate_exec_exit(&exec_id, &result, Vec::new())
        .expect("missing diag maps to empty archive");
    assert_eq!(archive, empty);
    let projection = super::super::projection::sanitize_archive(&archive)
        .expect("empty archive is a valid absent-log projection");
    assert!(
        projection
            .windows(b"\"runner_log_present\":false".len())
            .any(|window| window == b"\"runner_log_present\":false")
    );
    assert!(
        projection
            .windows(b"\"worker_log_present\":false".len())
            .any(|window| window == b"\"worker_log_present\":false")
    );

    let failure = ExecInspectResponse {
        id: Some(exec_id.clone()),
        running: Some(false),
        exit_code: Some(24),
        ..Default::default()
    };
    assert_eq!(
        validate_exec_exit(&exec_id, &failure, Vec::new()),
        Err(HostError::Docker)
    );
}

#[test]
fn exit_success_requires_the_exact_completed_exec_and_bounded_archive() {
    let exec_id = "a".repeat(64);
    assert!(validate_exec_id(&exec_id).is_ok());
    assert_eq!(
        validate_exec_id("../not-an-exec"),
        Err(HostError::Ownership)
    );
    let completed = ExecInspectResponse {
        id: Some(exec_id.clone()),
        running: Some(false),
        exit_code: Some(0),
        ..Default::default()
    };
    assert_eq!(
        validate_exec_exit(&"c".repeat(64), &completed, vec![1]),
        Err(HostError::Docker)
    );
    let running = ExecInspectResponse {
        running: Some(true),
        ..completed.clone()
    };
    assert_eq!(
        validate_exec_exit(&exec_id, &running, vec![1]),
        Err(HostError::Docker)
    );
    assert_eq!(
        validate_exec_exit(&exec_id, &completed, Vec::new()),
        Err(HostError::Docker)
    );
}

fn attached(items: Vec<Result<LogOutput, bollard::errors::Error>>) -> StartExecResults {
    StartExecResults::Attached {
        output: Box::pin(stream::iter(items)),
        input: Box::pin(tokio::io::sink()),
    }
}
