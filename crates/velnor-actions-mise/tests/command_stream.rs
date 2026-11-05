// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[test]
fn quiescent_file_stream_preserves_binary_stdout_and_bounded_stderr()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let mut output = create_output(&directory.path().join("stdout.bin"))?;
    let command = shell("printf 'A\\000B'; printf err >&2")?;

    let (result, _drained) = command.run_quiescent_stdout_to_file_until(
        &mut output,
        3,
        3,
        CheckDeadline::after(Duration::from_secs(2))?,
    )?;

    assert!(result.success);
    assert_eq!(result.code, Some(0));
    assert_eq!(result.signal, None);
    assert_eq!(result.stdout_bytes, 3);
    assert_eq!(result.stderr, b"err");
    drop(output);
    assert_eq!(std::fs::read(directory.path().join("stdout.bin"))?, b"A\0B");
    directory.close()?;
    Ok(())
}

#[test]
fn quiescent_file_stream_cap_failure_drains_descendants() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new()?;
    let marker = directory.path().join("descendant-ran");
    let mut output = create_output(&directory.path().join("stdout.bin"))?;
    let command = shell_with_arg(
        "(sleep 1; printf x > \"$1\") & printf overflow",
        marker.as_os_str(),
    )?;

    let error = command
        .run_quiescent_stdout_to_file_until(
            &mut output,
            1,
            32,
            CheckDeadline::after(Duration::from_secs(3))?,
        )
        .err()
        .ok_or_else(|| io::Error::other("expected stdout cap failure"))?;

    assert!(error.to_string().contains("stdout_limit_exceeded:1"));
    std::thread::sleep(Duration::from_millis(1_200));
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn quiescent_file_stream_success_drains_detached_descendants()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let marker = directory.path().join("descendant-ran");
    let output_path = directory.path().join("stdout.bin");
    let mut output = create_output(&output_path)?;
    let command = shell_with_arg(
        "(sleep 1; printf x > \"$1\") >/dev/null 2>&1 </dev/null & printf done",
        marker.as_os_str(),
    )?;

    let (result, _drained) = command.run_quiescent_stdout_to_file_until(
        &mut output,
        4,
        32,
        CheckDeadline::after(Duration::from_secs(3))?,
    )?;

    assert!(result.success);
    assert_eq!(result.code, Some(0));
    assert_eq!(result.stdout_bytes, 4);
    drop(output);
    assert_eq!(std::fs::read(output_path)?, b"done");
    std::thread::sleep(Duration::from_millis(1_200));
    assert!(!marker.exists());
    directory.close()?;
    Ok(())
}

#[test]
fn quiescent_file_stream_deadline_drains_descendants() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let marker = directory.path().join("descendant-ran");
    let mut output = create_output(&directory.path().join("stdout.bin"))?;
    let command = shell_with_arg("sleep 1; printf x > \"$1\"", marker.as_os_str())?;

    let error = command
        .run_quiescent_stdout_to_file_until(
            &mut output,
            32,
            32,
            CheckDeadline::after(Duration::from_millis(500))?,
        )
        .err()
        .ok_or_else(|| io::Error::other("expected deadline failure"))?;

    assert!(
        error
            .to_string()
            .contains("timeout_after_absolute_deadline")
    );
    std::thread::sleep(Duration::from_millis(1_200));
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn quiescent_file_stream_stderr_cap_failure_is_reported() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new()?;
    let mut output = create_output(&directory.path().join("stdout.bin"))?;
    let command = shell("printf stderr-overflow >&2")?;

    let error = command
        .run_quiescent_stdout_to_file_until(
            &mut output,
            32,
            2,
            CheckDeadline::after(Duration::from_secs(2))?,
        )
        .err()
        .ok_or_else(|| io::Error::other("expected stderr cap failure"))?;

    assert!(error.to_string().contains("stderr_limit_exceeded:2"));
    Ok(())
}

fn shell(script: &str) -> Result<IsolatedCommand, velnor_actions_mise::MiseError> {
    IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
}

fn shell_with_arg(
    script: &str,
    argument: &std::ffi::OsStr,
) -> Result<IsolatedCommand, velnor_actions_mise::MiseError> {
    IsolatedCommand::repo_task(
        "/bin/sh",
        vec![
            OsString::from("-c"),
            OsString::from(script),
            OsString::from("fixture"),
            argument.to_owned(),
        ],
        &[],
    )
}

fn create_output(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-mise-command-stream-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            remove_on_drop: true,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn close(mut self) -> io::Result<()> {
        std::fs::remove_dir_all(&self.path)?;
        self.remove_on_drop = false;
        Ok(())
    }
}

struct TestDirectory {
    path: PathBuf,
    remove_on_drop: bool,
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.remove_on_drop {
            drop(std::fs::remove_dir_all(&self.path));
        }
    }
}
