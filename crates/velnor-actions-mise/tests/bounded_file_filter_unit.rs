use super::process::{group_signal_test_hook, run_filter_io};
use crate::command::command_for_file_filter as make_file_filter_command;
use std::fs::File;
use std::io;
use std::io::{Cursor, Error, Read, Write};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

struct TestRoot(std::path::PathBuf);

impl TestRoot {
    fn new() -> io::Result<Self> {
        crate::test_temp_dir::unique_temp_dir("velnor-bounded-filter-test").map(Self)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("bounded_filter_test_cleanup_failed:{:?}", error.kind());
        }
    }
}

fn command_for_file_filter(program: &str, args: &[&str]) -> Command {
    make_file_filter_command(std::ffi::OsStr::new(program), args)
}

#[test]
fn file_filter_rejects_nonregular_streams_before_spawn() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new()?;
    let output_path = root.path().join("output");
    let mut output = File::create(&output_path)?;
    let directory = File::open(root.path())?;
    let input_error = crate::run_zstd_file_filter(
        std::path::Path::new("/usr/bin/true"),
        crate::ZstdFilterMode::Decode,
        directory,
        &mut output,
        1_024,
        1_024,
        Duration::from_secs(1),
    );
    assert_eq!(input_error, Err("cache_tar_archive_invalid"));

    let input_path = root.path().join("input");
    File::create(&input_path)?;
    let input = File::open(input_path)?;
    let output_directory = File::open(root.path())?;
    let output_error = crate::run_zstd_file_filter(
        std::path::Path::new("/usr/bin/true"),
        crate::ZstdFilterMode::Encode,
        input,
        &mut output_directory.try_clone()?,
        1_024,
        1_024,
        Duration::from_secs(1),
    );
    assert_eq!(output_error, Err("cache_tar_archive_invalid"));
    Ok(())
}

#[test]
fn file_filter_factory_clears_inherited_environment() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = command_for_file_filter("/usr/bin/env", &[]);
    let mut output = Vec::new();
    run_filter_io(
        &mut command,
        Cursor::new(b""),
        &mut output,
        1_024,
        1_024,
        Duration::from_secs(2),
    )
    .map_err(io::Error::other)?;
    assert!(
        output.is_empty(),
        "file filters receive no inherited environment"
    );
    Ok(())
}

#[test]
fn zstd_input_read_failure_kills_and_reaps_child() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new()?;
    let pid_path = root.path().join("pid");
    let script = format!(
        "echo $$ > '{}'; cat >/dev/null; exec /bin/sleep 30",
        pid_path.display()
    );
    let mut command = command_for_file_filter("/bin/sh", &["-c", &script]);
    let error = run_filter_io(
        &mut command,
        DelayedFailureReader {
            pid_path: pid_path.clone(),
        },
        &mut Vec::new(),
        1_024,
        1_024,
        Duration::from_secs(2),
    );
    let Err(error) = error else {
        return Err(io::Error::other("input failure must fail the filter").into());
    };
    assert_eq!(error, "cache_tar_archive_read_failed");
    assert_child_reaped(&pid_path)?;
    Ok(())
}

#[test]
fn zstd_output_write_failure_kills_and_reaps_child() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new()?;
    let pid_path = root.path().join("pid");
    let script = format!(
        "echo $$ > '{}'; printf x; exec /bin/sleep 30",
        pid_path.display()
    );
    let mut command = command_for_file_filter("/bin/sh", &["-c", &script]);
    let error = run_filter_io(
        &mut command,
        Cursor::new(b"input"),
        &mut DelayedFailureWriter {
            pid_path: pid_path.clone(),
        },
        1_024,
        1_024,
        Duration::from_secs(2),
    );
    let Err(error) = error else {
        return Err(io::Error::other("output failure must fail the filter").into());
    };
    assert_eq!(error, "cache_tar_archive_write_failed");
    assert_child_reaped(&pid_path)?;
    Ok(())
}

#[test]
fn zstd_output_limit_kills_and_reaps_child() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new()?;
    let pid_path = root.path().join("pid");
    let script = format!(
        "echo $$ > '{}'; printf 123456789; exec /bin/sleep 30",
        pid_path.display()
    );
    let mut command = command_for_file_filter("/bin/sh", &["-c", &script]);
    let error = run_filter_io(
        &mut command,
        Cursor::new(b"input"),
        &mut Vec::new(),
        1_024,
        4,
        Duration::from_secs(2),
    );
    let Err(error) = error else {
        return Err(io::Error::other("output above its limit must fail").into());
    };
    assert_eq!(error, "cache_tar_archive_limit");
    assert_child_reaped(&pid_path)?;
    Ok(())
}

#[test]
fn zstd_hung_child_is_stopped_and_reaped_at_deadline() -> Result<(), Box<dyn std::error::Error>> {
    let root = TestRoot::new()?;
    let pid_path = root.path().join("pid");
    let script = format!("echo $$ > '{}'; exec /bin/sleep 30", pid_path.display());
    let mut command = command_for_file_filter("/bin/sh", &["-c", &script]);
    let started = Instant::now();
    let error = run_filter_io(
        &mut command,
        Cursor::new(b""),
        &mut Vec::new(),
        1_024,
        1_024,
        Duration::from_millis(200),
    );
    let Err(error) = error else {
        return Err(io::Error::other("a hung filter must hit its deadline").into());
    };
    assert_eq!(error, "cache_tar_compression_timeout");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_child_reaped(&pid_path)?;
    Ok(())
}

#[test]
fn zstd_filter_copies_both_streams_and_reaps_normal_exit() -> Result<(), Box<dyn std::error::Error>>
{
    let mut command = command_for_file_filter("/bin/cat", &[]);
    let mut output = Vec::new();
    run_filter_io(
        &mut command,
        Cursor::new(b"bounded zstd stream"),
        &mut output,
        1_024,
        1_024,
        Duration::from_secs(2),
    )
    .map_err(io::Error::other)?;
    assert_eq!(output, b"bounded zstd stream");
    Ok(())
}

#[test]
fn zstd_filter_waits_for_exit_after_both_streams_close() {
    let mut command = command_for_file_filter(
        "/bin/sh",
        &["-c", "cat; exec 1>&-; exec 0<&-; exec /bin/sleep 0.1"],
    );
    let mut output = Vec::new();
    let result = run_filter_io(
        &mut command,
        Cursor::new(b"complete stream"),
        &mut output,
        1_024,
        1_024,
        Duration::from_secs(2),
    );
    assert_eq!(output, b"complete stream");
    assert!(
        result.is_ok(),
        "successful child failed after pipe EOF: {result:?}"
    );
}

#[test]
fn zstd_filter_keeps_leader_unreaped_until_group_cleanup() {
    struct DelayedFailure;

    impl Read for DelayedFailure {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            thread::sleep(Duration::from_millis(150));
            Err(Error::other("synthetic input failure"))
        }
    }

    group_signal_test_hook::enable();
    let result = run_filter_io(
        &mut command_for_file_filter("/usr/bin/true", &[]),
        DelayedFailure,
        &mut Vec::new(),
        1_024,
        1_024,
        Duration::from_secs(2),
    );
    let leader_was_already_reaped = group_signal_test_hook::leader_was_already_reaped();
    group_signal_test_hook::disable();

    assert!(result.is_err());
    assert!(
        !leader_was_already_reaped,
        "cleanup reached group signaling after the leader had been reaped"
    );
}

struct DelayedFailureReader {
    pid_path: std::path::PathBuf,
}

impl Read for DelayedFailureReader {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        if !wait_for_child(&self.pid_path) {
            return Err(Error::other("child did not record its pid"));
        }
        Err(Error::other("synthetic input failure"))
    }
}

struct DelayedFailureWriter {
    pid_path: std::path::PathBuf,
}

impl Write for DelayedFailureWriter {
    fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
        if !wait_for_child(&self.pid_path) {
            return Err(Error::other("child did not record its pid"));
        }
        Err(Error::other("synthetic output failure"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn wait_for_child(pid_path: &std::path::Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    while !pid_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    pid_path.exists()
}

fn assert_child_reaped(pid_path: &std::path::Path) -> io::Result<()> {
    let pid = std::fs::read_to_string(pid_path)?;
    let status = Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .status()?;
    if status.success() {
        return Err(io::Error::other(format!(
            "child {} remained alive",
            pid.trim()
        )));
    }
    Ok(())
}
