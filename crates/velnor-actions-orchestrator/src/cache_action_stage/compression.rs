use super::StageError;
use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const ZSTD_TIMEOUT: Duration = Duration::from_secs(300);
const PIPE_POLL_INTERVAL: Duration = Duration::from_millis(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveCompression {
    None,
    Gzip,
    Zstd,
    ZstdLong,
}

pub(crate) fn decode_archive(
    archive: File,
    compression: ArchiveCompression,
    zstd_path: Option<&Path>,
) -> Result<File, StageError> {
    let metadata = archive
        .metadata()
        .map_err(|_| "cache_tar_archive_open_failed")?;
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err("cache_tar_archive_invalid");
    }
    let mut decoded = tempfile::tempfile().map_err(|_| "cache_tar_temporary_file_failed")?;
    match compression {
        ArchiveCompression::None => copy_bounded(archive, &mut decoded)?,
        ArchiveCompression::Gzip => {
            let mut reader = MultiGzDecoder::new(BufReader::new(archive));
            copy_bounded(&mut reader, &mut decoded).map_err(|_| "cache_tar_compression_failed")?;
            let mut source = reader.into_inner();
            if !source
                .fill_buf()
                .map_err(|_| "cache_tar_compression_failed")?
                .is_empty()
                || source
                    .stream_position()
                    .map_err(|_| "cache_tar_compression_failed")?
                    != metadata.len()
            {
                return Err("cache_tar_compression_trailing_data");
            }
        }
        ArchiveCompression::Zstd | ArchiveCompression::ZstdLong => {
            let program = checked_zstd(zstd_path)?;
            let long_mode = if compression == ArchiveCompression::ZstdLong {
                "--long=30"
            } else {
                "--no-progress"
            };
            run_filter(
                Command::new(program).args(["-d", "-q", "-c", long_mode]),
                archive,
                &mut decoded,
                MAX_ARCHIVE_BYTES,
                MAX_ARCHIVE_BYTES,
                ZSTD_TIMEOUT,
            )?;
        }
    }
    decoded
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    Ok(decoded)
}

pub(crate) fn encode_archive(
    mut raw: File,
    output: &mut File,
    compression: ArchiveCompression,
    zstd_path: Option<&Path>,
) -> Result<(), StageError> {
    let length = raw
        .metadata()
        .map_err(|_| "cache_tar_temporary_file_failed")?
        .len();
    if length > MAX_ARCHIVE_BYTES {
        return Err("cache_tar_archive_limit");
    }
    match compression {
        ArchiveCompression::None => copy_bounded(raw, output),
        ArchiveCompression::Gzip => {
            let mut encoder = GzEncoder::new(output, Compression::default());
            copy_bounded(raw, &mut encoder)?;
            encoder
                .finish()
                .map(|_| ())
                .map_err(|_| "cache_tar_archive_write_failed")
        }
        ArchiveCompression::Zstd | ArchiveCompression::ZstdLong => {
            let program = checked_zstd(zstd_path)?;
            raw.seek(SeekFrom::Start(0))
                .map_err(|_| "cache_tar_temporary_file_failed")?;
            let long_mode = if compression == ArchiveCompression::ZstdLong {
                "--long=30"
            } else {
                "--no-progress"
            };
            run_filter(
                Command::new(program).args(["-q", "-T0", "-c", long_mode]),
                raw,
                output,
                MAX_ARCHIVE_BYTES,
                MAX_ARCHIVE_BYTES,
                ZSTD_TIMEOUT,
            )
        }
    }
}

enum FilterProgress {
    Input(Result<(), StageError>),
    Output(Result<(), StageError>),
}

fn run_filter<R, W>(
    command: &mut Command,
    input: R,
    output: &mut W,
    input_limit: u64,
    output_limit: u64,
    timeout: Duration,
) -> Result<(), StageError>
where
    R: Read + Send,
    W: Write + Send,
{
    use std::os::unix::process::CommandExt;

    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or("cache_tar_compression_timeout")?;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| "cache_tar_compression_failed")?;
    let group = child.id();
    let Some(child_input) = child.stdin.take() else {
        terminate_and_reap(&mut child, group);
        return Err("cache_tar_compression_failed");
    };
    let Some(child_output) = child.stdout.take() else {
        drop(child_input);
        terminate_and_reap(&mut child, group);
        return Err("cache_tar_compression_failed");
    };
    if set_nonblocking(&child_input).is_err() || set_nonblocking(&child_output).is_err() {
        drop(child_input);
        drop(child_output);
        terminate_and_reap(&mut child, group);
        return Err("cache_tar_compression_failed");
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    transfer_and_wait(FilterJob {
        child: &mut child,
        group,
        input,
        output,
        child_input,
        child_output,
        input_limit,
        output_limit,
        deadline,
        cancelled,
    })
}

struct FilterJob<'a, R, W> {
    child: &'a mut Child,
    group: u32,
    input: R,
    output: &'a mut W,
    child_input: std::process::ChildStdin,
    child_output: std::process::ChildStdout,
    input_limit: u64,
    output_limit: u64,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

fn transfer_and_wait<R, W>(job: FilterJob<'_, R, W>) -> Result<(), StageError>
where
    R: Read + Send,
    W: Write + Send,
{
    let FilterJob {
        child,
        group,
        input,
        output,
        mut child_input,
        mut child_output,
        input_limit,
        output_limit,
        deadline,
        cancelled,
    } = job;
    let (sender, receiver) = mpsc::channel();

    thread::scope(|scope| {
        let input_cancelled = Arc::clone(&cancelled);
        let input_sender = sender.clone();
        scope.spawn(move || {
            let result = copy_into_child(input, &mut child_input, input_limit, &input_cancelled);
            drop(input_sender.send(FilterProgress::Input(result)));
        });
        let output_cancelled = Arc::clone(&cancelled);
        scope.spawn(move || {
            let result =
                copy_from_child(&mut child_output, output, output_limit, &output_cancelled);
            drop(sender.send(FilterProgress::Output(result)));
        });

        let mut input_done = false;
        let mut output_done = false;
        let mut exit_status = None;
        let mut failure = None;
        while !(input_done && output_done && exit_status.is_some()) {
            match receiver.try_recv() {
                Ok(FilterProgress::Input(Ok(()))) => input_done = true,
                Ok(FilterProgress::Output(Ok(()))) => output_done = true,
                Ok(FilterProgress::Input(Err(problem)) | FilterProgress::Output(Err(problem))) => {
                    failure = Some(problem);
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    failure = Some("cache_tar_compression_failed");
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if exit_status.is_none() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            failure = Some("cache_tar_compression_failed");
                            break;
                        }
                        exit_status = Some(status);
                    }
                    Ok(None) => {}
                    Err(_) => {
                        failure = Some("cache_tar_compression_failed");
                        break;
                    }
                }
            }
            if Instant::now() >= deadline {
                failure = Some("cache_tar_compression_timeout");
                break;
            }
            thread::sleep(PIPE_POLL_INTERVAL);
        }

        if let Some(problem) = failure {
            cancelled.store(true, Ordering::Release);
            terminate_and_reap(child, group);
            Err(problem)
        } else {
            Ok(())
        }
    })
}

fn copy_into_child<R: Read, W: Write>(
    mut input: R,
    child_input: &mut W,
    limit: u64,
    cancelled: &AtomicBool,
) -> Result<(), StageError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("cache_tar_compression_failed");
        }
        let count = input
            .read(&mut buffer)
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            return Ok(());
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_limit")?)
            .filter(|total| *total <= limit)
            .ok_or("cache_tar_archive_limit")?;
        write_nonblocking(child_input, &buffer[..count], cancelled)?;
    }
}

fn copy_from_child<W: Write>(
    child_output: &mut impl Read,
    output: &mut W,
    limit: u64,
    cancelled: &AtomicBool,
) -> Result<(), StageError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("cache_tar_compression_failed");
        }
        match child_output.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                total = total
                    .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_limit")?)
                    .filter(|total| *total <= limit)
                    .ok_or("cache_tar_archive_limit")?;
                output
                    .write_all(&buffer[..count])
                    .map_err(|_| "cache_tar_archive_write_failed")?;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(PIPE_POLL_INTERVAL);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err("cache_tar_compression_failed"),
        }
    }
}

fn write_nonblocking<W: Write>(
    writer: &mut W,
    mut bytes: &[u8],
    cancelled: &AtomicBool,
) -> Result<(), StageError> {
    while !bytes.is_empty() {
        if cancelled.load(Ordering::Acquire) {
            return Err("cache_tar_compression_failed");
        }
        match writer.write(bytes) {
            Ok(0) => return Err("cache_tar_compression_failed"),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(PIPE_POLL_INTERVAL);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err("cache_tar_compression_failed"),
        }
    }
    Ok(())
}

fn set_nonblocking(fd: &impl std::os::fd::AsFd) -> Result<(), ()> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

    let flags = fcntl_getfl(fd).map_err(|_| ())?;
    fcntl_setfl(fd, flags | OFlags::NONBLOCK).map_err(|_| ())
}

fn terminate_and_reap(child: &mut Child, group: u32) {
    use rustix::io::Errno;
    use rustix::process::{Pid, Signal, kill_process_group};

    if let Ok(raw) = i32::try_from(group)
        && let Some(pid) = Pid::from_raw(raw)
        && let Err(error) = kill_process_group(pid, Signal::KILL)
        && error != Errno::SRCH
    {
        drop(child.kill());
    }
    drop(child.wait());
}

fn checked_zstd(path: Option<&Path>) -> Result<&Path, StageError> {
    let path = path.ok_or("cache_tar_compression_unavailable")?;
    if !path.is_absolute() || path.as_os_str().len() > 4_096 {
        return Err("cache_tar_compression_unavailable");
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "cache_tar_compression_unavailable")?;
    if !metadata.file_type().is_file() {
        return Err("cache_tar_compression_unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("cache_tar_compression_unavailable");
        }
    }
    Ok(path)
}

fn copy_bounded<R: Read, W: Write>(mut reader: R, writer: &mut W) -> Result<(), StageError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_limit")?)
            .ok_or("cache_tar_archive_limit")?;
        if total > MAX_ARCHIVE_BYTES {
            return Err("cache_tar_archive_limit");
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|_| "cache_tar_archive_write_failed")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ArchiveCompression, decode_archive, run_filter};
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
        let error = run_filter(
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
        let error = run_filter(
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
        let error = run_filter(
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
        let error = run_filter(
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
        run_filter(
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
}
