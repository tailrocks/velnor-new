// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const COMMON_STATUSES: &[&str] = &[
    "cargo-prefetch",
    "archive-guard-build",
    "validator-tool-install",
    "validator-tool-lock-admission",
    "cli-build",
    "external-capture",
];

const JOB_STATUSES: &[(&str, &[&str])] = &[
    (
        "ordinary-harness",
        &[
            "external-shell-harness",
            "external-shell-harness-json-proof",
        ],
    ),
    (
        "keep-harness",
        &[
            "external-shell-harness-keep",
            "external-shell-harness-keep-json-proof",
        ],
    ),
    (
        "generation-checks",
        &[
            "external-check",
            "rust-verifier-retry",
            "repository-oracle-check",
        ],
    ),
];

fn required_statuses() -> Vec<(&'static str, &'static str)> {
    JOB_STATUSES
        .iter()
        .flat_map(|(job, own)| {
            COMMON_STATUSES
                .iter()
                .chain(own.iter())
                .map(move |name| (*job, *name))
        })
        .collect()
}

#[test]
fn phase_dispatch_rejects_unknown_and_extra_arguments_before_side_effects()
-> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let unknown = run_phase(temporary.path(), &["unknown-phase"])?;
    assert_eq!(unknown.status.code(), Some(64));
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("unknown source qualification phase")
    );
    assert!(
        !temporary
            .path()
            .join("source-qualification-artifacts")
            .exists()
    );

    let extra = run_phase(
        temporary.path(),
        &["prepare-external-evidence-directories", "unexpected"],
    )?;
    assert_eq!(extra.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&extra.stderr).contains("usage:"));
    assert!(
        !temporary
            .path()
            .join("source-qualification-artifacts")
            .exists()
    );
    Ok(())
}

#[test]
fn aggregation_accepts_complete_success_receipts_and_retains_result() -> Result<(), Box<dyn Error>>
{
    let temporary = tempfile::tempdir()?;
    write_receipts(temporary.path(), None, None)?;
    for (job, _) in JOB_STATUSES {
        fs::write(
            jobs_dir(temporary.path())
                .join(job)
                .join("cli-identity.txt"),
            "x\n",
        )?;
    }
    let output = run_phase(
        temporary.path(),
        &["bundle-selected-qualification-evidence"],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence = evidence_dir(temporary.path());
    assert_eq!(
        fs::read_to_string(evidence.join("qualification-final-status.txt"))?,
        "qualification_checks_status=0\n"
    );
    assert_eq!(
        fs::read_to_string(evidence.join("logs/evidence-bundle.exit"))?,
        "0\n"
    );
    let archive = evidence.join("source-qualification-preserved.tar.gz");
    let members = archive_members(&archive)?;
    for (job, _) in JOB_STATUSES {
        for path in [format!("{job}/logs/"), format!("{job}/cli-identity.txt")] {
            assert!(
                members.iter().any(|member| member == &path),
                "archive has no member {path}: {members:?}"
            );
        }
    }
    assert!(
        members
            .iter()
            .any(|member| member == "qualification-final-status.txt")
    );
    assert!(
        members
            .iter()
            .all(|member| member == "qualification-final-status.txt"
                || JOB_STATUSES
                    .iter()
                    .any(|(job, _)| member.starts_with(&format!("{job}/")))),
        "archive has an unprefixed member: {members:?}"
    );
    Ok(())
}

#[test]
fn aggregation_fails_and_retains_status_for_every_missing_or_failed_status()
-> Result<(), Box<dyn Error>> {
    for (job, name) in required_statuses() {
        for (failed, missing) in [(Some((job, name)), None), (None, Some((job, name)))] {
            let temporary = tempfile::tempdir()?;
            write_receipts(temporary.path(), failed, missing)?;
            assert_aggregation_failed(temporary.path(), &format!("job {job} status {name} "))?;
        }
    }
    Ok(())
}

#[test]
fn aggregation_fails_for_unsafe_receipts_and_missing_job_evidence() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    write_receipts(
        temporary.path(),
        None,
        Some(("generation-checks", "root-preview")),
    )?;
    assert_aggregation_failed(
        temporary.path(),
        "job generation-checks status root-preview ",
    )?;

    let temporary = tempfile::tempdir()?;
    write_receipts(
        temporary.path(),
        None,
        Some(("generation-checks", "root-preview")),
    )?;
    let logs = jobs_dir(temporary.path()).join("generation-checks/logs");
    #[cfg(unix)]
    std::os::unix::fs::symlink("../missing", logs.join("root-preview.exit"))?;
    #[cfg(not(unix))]
    return Err("unsafe receipt test requires symlink support".into());
    assert_aggregation_failed(
        temporary.path(),
        "job generation-checks status root-preview ",
    )?;

    for failing in [
        "generate_statuses=1,0 identity_status=0 diff_statuses=0,0\n",
        "generate_statuses=0,0 identity_status=1 diff_statuses=0,0\n",
        "generate_statuses=0,0 identity_status=0 diff_statuses=0,1\n",
    ] {
        let temporary = tempfile::tempdir()?;
        write_receipts(temporary.path(), None, None)?;
        fs::write(
            jobs_dir(temporary.path()).join("generation-checks/logs/root-preview.exit"),
            failing,
        )?;
        assert_aggregation_failed(
            temporary.path(),
            "job generation-checks status root-preview ",
        )?;
    }

    let temporary = tempfile::tempdir()?;
    write_receipts(temporary.path(), None, None)?;
    let logs = jobs_dir(temporary.path()).join("keep-harness/logs");
    fs::remove_file(logs.join("cli-build.exit"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("../missing", logs.join("cli-build.exit"))?;
    assert_aggregation_failed(temporary.path(), "job keep-harness status cli-build ")?;

    let temporary = tempfile::tempdir()?;
    write_receipts(temporary.path(), None, None)?;
    fs::remove_dir_all(jobs_dir(temporary.path()).join("ordinary-harness"))?;
    let stderr = assert_aggregation_failed(
        temporary.path(),
        "evidence for job ordinary-harness is missing or unsafe",
    )?;
    assert!(
        stderr.contains("Re-run all jobs"),
        "a partial re-run is not told to re-run all jobs: {stderr}"
    );
    Ok(())
}

#[test]
fn aggregation_checks_each_job_evidence_independently() -> Result<(), Box<dyn Error>> {
    for (job, _) in JOB_STATUSES {
        let temporary = tempfile::tempdir()?;
        write_receipts(temporary.path(), Some((job, "cli-build")), None)?;
        let stderr =
            assert_aggregation_failed(temporary.path(), &format!("job {job} status cli-build "))?;
        for (other, _) in JOB_STATUSES.iter().filter(|(other, _)| other != job) {
            assert!(
                !stderr.contains(&format!("job {other} ")),
                "job {other} was blamed for a failure in job {job}: {stderr}"
            );
        }
    }
    Ok(())
}

fn assert_aggregation_failed(root: &Path, expected_stderr: &str) -> Result<String, Box<dyn Error>> {
    let output = run_phase(root, &["bundle-selected-qualification-evidence"])?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(1), "{expected_stderr}: {stderr}");
    assert!(
        stderr.contains(expected_stderr),
        "{expected_stderr}: {stderr}"
    );
    let evidence = evidence_dir(root);
    assert_eq!(
        fs::read_to_string(evidence.join("qualification-final-status.txt"))?,
        "qualification_checks_status=1\n"
    );
    assert_eq!(
        fs::read_to_string(evidence.join("logs/evidence-bundle.exit"))?,
        "0\n"
    );
    assert!(
        evidence
            .join("source-qualification-preserved.tar.gz")
            .is_file()
    );
    Ok(stderr)
}

fn write_receipts(
    root: &Path,
    failed: Option<(&str, &str)>,
    missing: Option<(&str, &str)>,
) -> Result<(), Box<dyn Error>> {
    for (job, own) in JOB_STATUSES {
        let logs = jobs_dir(root).join(job).join("logs");
        fs::create_dir_all(&logs)?;
        for name in COMMON_STATUSES.iter().chain(own.iter()) {
            if missing == Some((*job, *name)) {
                continue;
            }
            let status = if failed == Some((*job, *name)) {
                "1\n"
            } else {
                "0\n"
            };
            fs::write(logs.join(format!("{name}.exit")), status)?;
        }
    }
    if missing != Some(("generation-checks", "root-preview")) {
        fs::write(
            jobs_dir(root).join("generation-checks/logs/root-preview.exit"),
            "generate_statuses=0,0 identity_status=0 diff_statuses=0,0\n",
        )?;
    }
    Ok(())
}

fn archive_members(archive: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("tar").arg("-tzf").arg(archive).output()?;
    assert!(output.status.success());
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect())
}

fn jobs_dir(root: &Path) -> PathBuf {
    root.join("source-qualification-jobs")
}

pub(super) fn evidence_dir(root: &Path) -> PathBuf {
    root.join("source-qualification-artifacts")
}

pub(super) fn run_phase(root: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification.sh");
    Ok(Command::new("bash")
        .arg(script)
        .args(args)
        .env("RUNNER_TEMP", root)
        .output()?)
}
