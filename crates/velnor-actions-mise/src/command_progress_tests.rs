//! Actual subprocess and encoding witnesses for opt-in live diagnostics.
use super::*;
use std::{
    ffi::OsString,
    io,
    time::{Duration, Instant},
};

fn task(script: &str) -> Result<IsolatedCommand, MiseError> {
    IsolatedCommand::repo_task("/bin/sh", vec![OsString::from("-c"), script.into()], &[])
}

#[derive(Default)]
struct Observations {
    bytes: Vec<u8>,
    writes: Vec<Instant>,
}

impl Write for Observations {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes.push(Instant::now());
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn both_streams_are_visible_before_deadline_failure_and_descendants_are_killed() {
    let root = crate::test_temp_dir::TestTempDir::new("velnor-progress").expect("private root");
    let marker = root.path().join("survived");
    let script = format!(
        "printf out; printf err >&2; (sleep 1; touch '{}') & sleep 30",
        marker.display()
    );
    let started = Instant::now();
    let deadline =
        CheckDeadline::from_start(started, Duration::from_millis(400)).expect("deadline");
    let mut observations = Observations::default();
    let result =
        task(&script)
            .expect("task")
            .run_until_with_progress(4096, deadline, &mut observations);
    assert!(result.is_err());
    let ended = Instant::now();
    let text = String::from_utf8(observations.bytes).expect("escaped ASCII");
    assert!(text.contains("velnor-progress stdout: out"), "{text}");
    assert!(text.contains("velnor-progress stderr: err"), "{text}");
    assert!(observations.writes.iter().all(|time| *time < ended));
    assert!(
        observations
            .writes
            .iter()
            .any(|time| time.duration_since(started) < Duration::from_millis(300))
    );
    assert!(ended.duration_since(started) < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(1100));
    assert!(!marker.exists(), "descendant must not survive the deadline");
}

#[test]
fn successful_capture_remains_exact_and_default_commands_have_no_observer() {
    let command = task("printf private-stdout; printf private-stderr >&2").expect("task");
    let plain = command
        .run_until(
            4096,
            CheckDeadline::after(Duration::from_secs(2)).expect("deadline"),
        )
        .expect("default capture");
    assert_eq!(plain.stdout, b"private-stdout");
    assert_eq!(plain.stderr, b"private-stderr");
    if std::env::var_os("VELNOR_PROGRESS_DEFAULT_CHILD").is_some() {
        return;
    }
    let executable = std::env::current_exe().expect("test binary");
    let child = IsolatedCommand::repo_task(
        executable.to_str().expect("test binary UTF-8 path"),
        ["--exact", "command::progress::tests::successful_capture_remains_exact_and_default_commands_have_no_observer", "--nocapture", "--test-threads=1"].into_iter().map(OsString::from).collect(),
        &[("VELNOR_PROGRESS_DEFAULT_CHILD".into(), "1".into())],
    ).expect("default capture witness");
    let outer = child
        .run_until(
            8192,
            CheckDeadline::after(Duration::from_secs(5)).expect("outer deadline"),
        )
        .expect("child capture");
    assert!(outer.success);
    assert!(String::from_utf8_lossy(&outer.stdout).contains("1 passed"));
    assert!(!String::from_utf8_lossy(&outer.stdout).contains("private-"));
    assert!(!String::from_utf8_lossy(&outer.stderr).contains("private-"));
    let mut observed = Vec::new();
    let live = command
        .run_until_with_progress(
            4096,
            CheckDeadline::after(Duration::from_secs(2)).expect("deadline"),
            &mut observed,
        )
        .expect("observed capture");
    assert_eq!(plain.stdout, live.stdout);
    assert_eq!(plain.stderr, live.stderr);
    assert!(live.success);
    assert_ne!(observed, [] as [u8; 0]);
}

#[test]
fn workflow_commands_controls_and_split_delimiters_are_escaped() {
    let mut output = Vec::new();
    let deadline = CheckDeadline::after(Duration::from_secs(2)).expect("deadline");
    let mut progress = ProgressSink::new(&mut output, deadline);
    for chunk in [
        &b"##[error]bad\n##[add-mask]hidden\r##[stop-commands]x"[..],
        &b"::error::bad\n::add-mask::hidden\n::stop-commands::x"[..],
        &b"\x1b[31m\0\t\x7f\xc2\x85\\"[..],
        &b"#"[..],
        &b"#[error]split"[..],
    ] {
        progress.chunk("stderr", chunk).expect("escaped progress");
    }
    let text = String::from_utf8(output).expect("ASCII");
    assert!(!text.contains("##["));
    assert!(text.contains("\\x23\\x23[error]"));
    assert!(text.contains("\\x1b[31m\\x00\\x09\\x7f\\xc2\\x85\\x5c"));
    assert!(
        text.lines()
            .all(|line| line.starts_with("velnor-progress stderr: "))
    );
    assert!(
        text.bytes()
            .all(|byte| byte == b'\n' || (b' '..=b'~').contains(&byte))
    );
}

#[test]
fn encoded_budget_includes_expansion_prefixes_and_one_explicit_truncation() {
    let mut output = Vec::new();
    let mut progress = ProgressSink::new(
        &mut output,
        CheckDeadline::after(Duration::from_secs(2)).expect("deadline"),
    );
    for _ in 0..100 {
        progress.chunk("stdout", &[0; 8192]).expect("progress");
    }
    progress
        .chunk("stderr", b"not a second marker")
        .expect("already truncated");
    assert!(output.len() <= MAX_PROGRESS_BYTES);
    let text = String::from_utf8(output).expect("ASCII");
    assert_eq!(text.matches("diagnostic limit reached").count(), 1);
    assert!(!text.contains("not a second marker"));
}

#[test]
fn observer_failures_and_capture_limits_preserve_process_failure() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("unavailable"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let error = task("printf x; sleep 30")
        .expect("task")
        .run_until_with_progress(
            4096,
            CheckDeadline::after(Duration::from_secs(2)).expect("deadline"),
            &mut Broken,
        )
        .expect_err("observer failure");
    assert!(error.to_string().contains("progress_write_failed"));
    for script in ["printf abcd", "printf abcd >&2"] {
        let mut output = Vec::new();
        let error = task(script)
            .expect("task")
            .run_until_with_progress(
                3,
                CheckDeadline::after(Duration::from_secs(2)).expect("deadline"),
                &mut output,
            )
            .expect_err("original cap");
        assert!(error.to_string().contains("_limit_exceeded:3"));
        assert!(output.is_empty(), "over-cap chunk is never published");
    }
}
