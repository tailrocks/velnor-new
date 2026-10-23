//! Real subprocess witnesses for the shared bounded file sink.
use super::*;
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
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream").expect("owned root");
    let mut file = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.path().join("stdout"))
        .expect("owned file");
    let output = task("printf abc; printf err >&2")
        .expect("fixed shell request")
        .run_stdout_to_file_until(
            &mut file,
            u64::MAX,
            3,
            deadline(2000).expect("bounded deadline"),
        )
        .expect("stream succeeds");
    assert!(output.success);
    assert_eq!(output.code, Some(0));
    assert_eq!(output.stdout_bytes, 3);
    assert_eq!(output.stderr, b"err");
    file.rewind().expect("rewind");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).expect("read retained file");
    assert_eq!(bytes, b"abc");
}

#[test]
fn stdout_file_enforces_both_stream_caps_and_write_errors() {
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream").expect("owned root");
    let mut file = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.path().join("stdout"))
        .expect("owned file");
    let error = task("printf abcd")
        .expect("fixed shell request")
        .run_stdout_to_file_until(&mut file, 3, 100, deadline(2000).expect("bounded deadline"))
        .expect_err("stdout cap");
    assert!(error.to_string().contains("stdout_limit_exceeded:3"));
    assert!(file.metadata().expect("metadata").len() <= 3);
    let error = task("printf abcd >&2")
        .expect("fixed shell request")
        .run_stdout_to_file_until(&mut file, 100, 3, deadline(2000).expect("bounded deadline"))
        .expect_err("independent stderr cap");
    assert!(error.to_string().contains("stderr_limit_exceeded:3"));
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream").expect("owned root");
    let path = dir.path().join("readonly");
    std::fs::write(&path, b"").expect("create file");
    let mut readonly = std::fs::File::open(path).expect("read-only regular descriptor");
    let error = task("printf abc; sleep 30")
        .expect("fixed shell request")
        .run_stdout_to_file_until(
            &mut readonly,
            100,
            100,
            deadline(2000).expect("bounded deadline"),
        )
        .expect_err("actual write failure kills child");
    assert!(error.to_string().contains("stdout_sink_write_failed:"));
}

#[test]
fn continuously_readable_stdout_cannot_starve_deadline_or_group_cleanup() {
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream").expect("owned root");
    let marker = dir.path().join("descendant-survived");
    let script = format!(
        "(sleep 0.4; printf survived > '{}') & while :; do printf abcdefghijklmnopqrstuvwxyz; done",
        marker.display()
    );
    let dir = crate::test_temp_dir::TestTempDir::new("velnor-stream").expect("owned root");
    let mut file = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.path().join("stdout"))
        .expect("owned file");
    let start = Instant::now();
    let error = task(&script)
        .expect("fixed shell request")
        .run_stdout_to_file_until(
            &mut file,
            u64::MAX,
            1024,
            deadline(100).expect("bounded deadline"),
        )
        .expect_err("continuous output must time out");
    assert!(super::super::is_cancel_or_timeout(&error));
    assert!(start.elapsed() < Duration::from_secs(3));
    std::thread::sleep(Duration::from_millis(600));
    assert!(!marker.exists(), "descendant process group must be killed");
}

#[test]
fn only_an_appended_cleanup_diagnostic_is_a_cleanup_failure() {
    let spawn = |message: &str| MiseError::SpawnFailed {
        program: "mise".to_owned(),
        message: message.to_owned(),
    };
    let is = super::super::is_cleanup_failure;
    assert!(is(&spawn(
        "timeout_after_secs:480;cleanup_failed:reap: EPERM"
    )));
    assert!(!is(&spawn("timeout_after_secs:480")));
    assert!(!is(&spawn("cancelled")));
    assert!(!is(&MiseError::EmptyToolchain));
}
