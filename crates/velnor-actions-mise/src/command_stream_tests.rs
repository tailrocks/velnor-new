//! Test bounded file output with real child processes.
use super::*;
use crate::CheckDeadline;
use std::ffi::OsString;
use std::io::{Read, Seek};
use std::time::{Duration, Instant};

fn task(script: &str) -> Result<IsolatedCommand, MiseError> {
    IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
}

fn deadline(milliseconds: u64) -> Result<CheckDeadline, MiseError> {
    CheckDeadline::after(Duration::from_millis(milliseconds))
}

#[test]
fn stdout_file_records_exact_bytes_with_independent_stderr() {
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream")
        .expect("The test directory must be created.");
    let mut file = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.path().join("stdout"))
        .expect("The output file must be created.");
    let output = task("printf abc; printf err >&2")
        .expect("The test shell command must be valid.")
        .run_stdout_to_file_until(
            &mut file,
            u64::MAX,
            3,
            deadline(2000).expect("The test deadline must be valid."),
        )
        .expect("The command must write output to the file.");
    assert!(output.success);
    assert_eq!(output.code, Some(0));
    assert_eq!(output.stdout_bytes, 3);
    assert_eq!(output.stderr, b"err");
    file.rewind().expect("The output file position must reset.");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .expect("The retained output file must be readable.");
    assert_eq!(bytes, b"abc");
}

#[test]
fn stdout_file_enforces_both_stream_caps_and_write_errors() {
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream")
        .expect("The test directory must be created.");
    let mut file = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.path().join("stdout"))
        .expect("The output file must be created.");
    let error = task("printf abcd")
        .expect("The test shell command must be valid.")
        .run_stdout_to_file_until(
            &mut file,
            3,
            100,
            deadline(2000).expect("The test deadline must be valid."),
        )
        .expect_err("The standard output limit must reject this command.");
    assert!(error.to_string().contains("stdout_limit_exceeded:3"));
    assert!(
        file.metadata()
            .expect("The output file metadata must be readable.")
            .len()
            <= 3
    );
    let error = task("printf abcd >&2")
        .expect("The test shell command must be valid.")
        .run_stdout_to_file_until(
            &mut file,
            100,
            3,
            deadline(2000).expect("The test deadline must be valid."),
        )
        .expect_err("The independent standard error limit must reject this command.");
    assert!(error.to_string().contains("stderr_limit_exceeded:3"));
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream")
        .expect("The test directory must be created.");
    let path = dir.path().join("readonly");
    std::fs::write(&path, b"").expect("The test file must be created.");
    let mut readonly = std::fs::File::open(path).expect("The test file must open for reading.");
    let error = task("printf abc; sleep 30")
        .expect("The test shell command must be valid.")
        .run_stdout_to_file_until(
            &mut readonly,
            100,
            100,
            deadline(2000).expect("The test deadline must be valid."),
        )
        .expect_err("The output file write must fail.");
    assert!(error.to_string().contains("stdout_sink_write_failed:"));
}

#[test]
fn continuously_readable_stdout_cannot_starve_deadline_or_group_cleanup() {
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream")
        .expect("The test directory must be created.");
    let marker = dir.path().join("descendant-survived");
    let script = format!(
        "(sleep 0.4; printf survived > '{}') & while :; do printf abcdefghijklmnopqrstuvwxyz; done",
        marker.display()
    );
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream")
        .expect("The test directory must be created.");
    let mut file = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.path().join("stdout"))
        .expect("The output file must be created.");
    let start = Instant::now();
    let error = task(&script)
        .expect("The test shell command must be valid.")
        .run_stdout_to_file_until(
            &mut file,
            u64::MAX,
            1024,
            deadline(100).expect("The test deadline must be valid."),
        )
        .expect_err("The command with continuous output must time out.");
    assert!(super::super::is_cancel_or_timeout(&error));
    assert!(start.elapsed() < Duration::from_secs(3));
    std::thread::sleep(Duration::from_millis(600));
    assert!(!marker.exists(), "The descendant process group must stop.");
}
