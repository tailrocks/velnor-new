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
