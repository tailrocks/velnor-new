// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use velnor_actions_mise::ImageCandidateCommand;

#[test]
fn image_build_drains_large_progress_and_preserves_output() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TestDirectory::new()?;
    let archive = directory.path().join("archive");
    let script = "head -c 150000 /dev/zero; head -c 150000 /dev/zero >&2; printf archive > \"$1\"";
    let output = ImageCandidateCommand::run_build(
        &shell_with_arg(script, archive.as_os_str())?,
        CheckDeadline::after(Duration::from_secs(10))?,
    )?;
    assert!(output.success);
    assert_eq!(std::fs::read(&archive)?, b"archive");
    assert_eq!(output.stdout.len(), 128 * 1024);
    assert_eq!(output.stderr.len(), 128 * 1024);
    Ok(())
}

#[test]
fn image_build_reports_failure_and_times_out_without_finishing()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let failed = ImageCandidateCommand::run_build(
        &shell("printf failure >&2; exit 7")?,
        CheckDeadline::after(Duration::from_secs(5))?,
    )
    .err()
    .ok_or("build failure accepted")?;
    assert!(
        matches!(
            failed,
            velnor_actions_mise::MiseError::NonZeroExit { code: Some(7), .. }
        ),
        "{failed}"
    );
    let marker = directory.path().join("late");
    let timed_out = ImageCandidateCommand::run_build(
        &shell_with_arg("sleep 2; printf late > \"$1\"", marker.as_os_str())?,
        CheckDeadline::after(Duration::from_secs(1))?,
    )
    .err()
    .ok_or("build timeout accepted")?;
    assert!(timed_out.to_string().contains("deadline"), "{timed_out}");
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !marker.exists(),
        "timed out build left an executing descendant"
    );
    Ok(())
}

#[test]
fn image_build_finishes_when_caller_stderr_is_not_drained() -> Result<(), Box<dyn std::error::Error>>
{
    const MARKER: &str = "VELNOR_TEST_BUILD_PROGRESS_MARKER";
    if let Some(path) = std::env::var_os(MARKER) {
        let output = ImageCandidateCommand::run_build(
            &shell_with_arg(
                "head -c 150000 /dev/zero; head -c 150000 /dev/zero >&2; printf archive > \"$1\"",
                &path,
            )?,
            CheckDeadline::after(Duration::from_secs(10))?,
        )?;
        assert!(output.success);
        return Ok(());
    }
    let directory = TestDirectory::new()?;
    let archive = directory.path().join("unread-stderr-archive");
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "command_stream::build_progress::image_build_finishes_when_caller_stderr_is_not_drained", "--nocapture"])
        .env(MARKER, &archive)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let until = std::time::Instant::now() + Duration::from_secs(15);
    let completed = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if std::time::Instant::now() >= until {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    if completed.is_none() {
        drop(child.stderr.take());
        child.wait()?;
    }
    assert!(
        completed.is_some_and(|status| status.success()),
        "build stalled or failed with unread stderr"
    );
    assert_eq!(std::fs::read(archive)?, b"archive");
    Ok(())
}
