use super::process::{group_signal_test_hook, run_filter_io};
use super::{ArchiveCompression, decode_archive};
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::{Cursor, Error, Read, Seek, SeekFrom, Write};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

fn gzip(input: &[u8]) -> std::fs::File {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input).expect("write gzip input");
    let bytes = encoder.finish().expect("finish gzip");
    let mut file = tempfile::tempfile().expect("temporary gzip");
    file.write_all(&bytes).expect("write compressed data");
    file.rewind().expect("rewind compressed data");
    file
}

#[test]
fn gzip_decoder_rejects_bytes_after_the_last_member() {
    let mut file = gzip(b"valid tar bytes");
    file.seek(SeekFrom::End(0)).expect("seek after gzip member");
    file.write_all(b"unparsed trailing bytes")
        .expect("append trailing bytes");
    file.rewind().expect("rewind compressed data");
    assert!(decode_archive(file, ArchiveCompression::Gzip, None).is_err());
}

#[test]
fn gzip_decoder_returns_the_complete_decompressed_stream() {
    let file = gzip(b"valid tar bytes");
    let mut decoded =
        decode_archive(file, ArchiveCompression::Gzip, None).expect("decode complete gzip");
    let mut bytes = Vec::new();
    decoded
        .read_to_end(&mut bytes)
        .expect("read decompressed data");
    assert_eq!(bytes, b"valid tar bytes");
}

#[test]
fn zstd_input_read_failure_kills_and_reaps_child() {
    let root = tempfile::tempdir().expect("temporary root");
    let pid_path = root.path().join("pid");
    let script = format!(
        "echo $$ > '{}'; cat >/dev/null; exec /bin/sleep 30",
        pid_path.display()
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &script]);
    let error = run_filter_io(
        &mut command,
        DelayedFailureReader {
            pid_path: pid_path.clone(),
        },
        &mut Vec::new(),
        1_024,
        1_024,
        Duration::from_secs(2),
    )
    .expect_err("input failure must fail the filter");
    assert_eq!(error, "cache_tar_archive_read_failed");
    assert_child_reaped(&pid_path);
}

#[test]
fn zstd_output_write_failure_kills_and_reaps_child() {
    let root = tempfile::tempdir().expect("temporary root");
    let pid_path = root.path().join("pid");
    let script = format!(
        "echo $$ > '{}'; printf x; exec /bin/sleep 30",
        pid_path.display()
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &script]);
    let error = run_filter_io(
        &mut command,
        Cursor::new(b"input"),
        &mut DelayedFailureWriter {
            pid_path: pid_path.clone(),
        },
        1_024,
        1_024,
        Duration::from_secs(2),
    )
    .expect_err("output failure must fail the filter");
    assert_eq!(error, "cache_tar_archive_write_failed");
    assert_child_reaped(&pid_path);
}

#[test]
fn zstd_output_limit_kills_and_reaps_child() {
    let root = tempfile::tempdir().expect("temporary root");
    let pid_path = root.path().join("pid");
    let script = format!(
        "echo $$ > '{}'; printf 123456789; exec /bin/sleep 30",
        pid_path.display()
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &script]);
    let error = run_filter_io(
        &mut command,
        Cursor::new(b"input"),
        &mut Vec::new(),
        1_024,
        4,
        Duration::from_secs(2),
    )
    .expect_err("output above its limit must fail");
    assert_eq!(error, "cache_tar_archive_limit");
    assert_child_reaped(&pid_path);
}

#[test]
fn zstd_hung_child_is_stopped_and_reaped_at_deadline() {
    let root = tempfile::tempdir().expect("temporary root");
    let pid_path = root.path().join("pid");
    let script = format!("echo $$ > '{}'; exec /bin/sleep 30", pid_path.display());
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &script]);
    let started = Instant::now();
    let error = run_filter_io(
        &mut command,
        Cursor::new(b""),
        &mut Vec::new(),
        1_024,
        1_024,
        Duration::from_millis(200),
    )
    .expect_err("a hung filter must hit its deadline");
    assert_eq!(error, "cache_tar_compression_timeout");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_child_reaped(&pid_path);
}

#[test]
fn zstd_filter_copies_both_streams_and_reaps_normal_exit() {
    let mut command = Command::new("/bin/cat");
    let mut output = Vec::new();
    run_filter_io(
        &mut command,
        Cursor::new(b"bounded zstd stream"),
        &mut output,
        1_024,
        1_024,
        Duration::from_secs(2),
    )
    .expect("successful filter");
    assert_eq!(output, b"bounded zstd stream");
}

#[test]
fn zstd_filter_waits_for_exit_after_both_streams_close() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "cat; exec 1>&-; exec 0<&-; exec /bin/sleep 0.1"]);
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
        &mut Command::new("/usr/bin/true"),
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

fn assert_child_reaped(pid_path: &std::path::Path) {
    let pid = std::fs::read_to_string(pid_path).expect("child wrote its pid");
    let status = Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .status()
        .expect("query child process");
    assert!(!status.success(), "child {} remained alive", pid.trim());
}
