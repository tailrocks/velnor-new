// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt;

use velnor_gen::{
    GenerateError, JobAuthorityError, expected_with_notes, generated_with_notes, io::view_of,
    job_authority_identity,
};
use velnor_model::{JOB_AUTHORITY_INVENTORY_PATH, Platform, RepoPath, RepoView};
use velnor_rust::{LOCK_PATH, RunningGenerator};

use super::job_authority_inventory::{CI, GATED, HELPER, enabled, pinned};
use super::{Fixture, fixture, git, text};
use crate::facts::TestResult;
use crate::lock_archive::archive_lock_at;

const OTHER: &str = "0123456789abcdef0123456789abcdef01234567";

pub(super) fn digest(view: &RepoView, platform: &str) -> Result<String, Box<dyn Error>> {
    let lock: toml::Table = toml::from_str(&String::from_utf8(view.files[&LOCK_PATH].clone())?)?;
    Ok(lock["velnor-gen-binary"][platform]
        .as_str()
        .ok_or("digest")?
        .to_owned())
}

pub(super) fn running(platform: Option<Platform>, sha: &str) -> RunningGenerator<'_> {
    RunningGenerator {
        version: HELPER,
        commit: pinned(),
        platform,
        sha256: Some(sha),
    }
}

fn refusal(view: &RepoView, running: RunningGenerator<'_>) -> Result<String, Box<dyn Error>> {
    match job_authority_identity(view, running) {
        Err(error) => Ok(error.to_string()),
        Ok(identity) => Err(format!("accepted: {identity:?}").into()),
    }
}

fn view() -> Result<RepoView, Box<dyn Error>> {
    enabled("full_run = \"off\"\n", None, &[CI])
}

#[test]
fn the_pinned_linux_generator_names_its_commit_and_executable_digest() -> TestResult {
    let view = view()?;
    for platform in [Platform::LinuxArm64, Platform::LinuxX64] {
        let sha = digest(&view, platform.file_suffix())?;
        let identity =
            job_authority_identity(&view, running(Some(platform), &sha))?.ok_or("no identity")?;
        assert_eq!(identity.lines(), format!("{}\n{sha}\n", pinned()));
    }
    let without = RepoView {
        files: view
            .files
            .iter()
            .filter(|(path, _)| path.as_str() != ".velnor/job-authority.toml")
            .map(|(path, bytes)| (path.clone(), bytes.clone()))
            .collect(),
        ..view
    };
    let sha = digest(&without, "linux-arm64")?;
    assert!(job_authority_identity(&without, running(Some(Platform::LinuxArm64), &sha))?.is_none());
    Ok(())
}

#[test]
fn a_generator_that_is_not_the_pinned_linux_binary_is_refused() -> TestResult {
    let view = view()?;
    let sha = digest(&view, "linux-arm64")?;
    let other_commit = |commit| RunningGenerator {
        commit,
        ..running(Some(Platform::LinuxArm64), &sha)
    };
    for commit in ["unknown", OTHER] {
        let message = refusal(&view, other_commit(commit))?;
        assert!(
            message.contains(commit) && message.contains(pinned()),
            "{commit}: {message}"
        );
    }
    let stamped_as_another_release = RunningGenerator {
        version: "0.1.116",
        ..running(Some(Platform::LinuxArm64), &sha)
    };
    assert!(refusal(&view, stamped_as_another_release)?.contains("version"));
    let forged = "f".repeat(64);
    assert!(refusal(&view, running(Some(Platform::LinuxArm64), &forged))?.contains(&forged));
    let wrong_platform = digest(&view, "linux-x64")?;
    let message = refusal(&view, running(Some(Platform::LinuxArm64), &wrong_platform))?;
    assert!(message.contains("digest"), "{message}");
    let macos = digest(&view, "macos-arm64")?;
    for platform in [Some(Platform::MacosArm64), None] {
        let message = refusal(&view, running(platform, &macos))?;
        assert!(
            message.contains("OrbStack linux") && message.contains("CI"),
            "{message}"
        );
    }
    Ok(())
}

#[test]
fn a_lock_without_the_velnor_gen_binary_digests_names_no_generator() -> TestResult {
    let mut view = view()?;
    view.files
        .insert(LOCK_PATH, archive_lock_at("0.1.110")?.into_bytes());
    let sha = "a".repeat(64);
    let old = RunningGenerator {
        version: "0.1.110",
        ..running(Some(Platform::LinuxArm64), &sha)
    };
    let message = refusal(&view, old)?;
    assert!(message.contains("binary digests"), "{message}");
    Ok(())
}

#[test]
fn more_workflows_than_the_host_accepts_are_refused_and_the_limit_itself_is_not() -> TestResult {
    let listed = |count: usize| -> Result<RepoView, Box<dyn Error>> {
        let paths: Vec<String> = (0..count)
            .map(|index| format!(".github/workflows/w{index}.yml"))
            .collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        enabled("full_run = \"off\"\n", None, &refs)
    };
    let over = expected_with_notes(&listed(33)?).err().ok_or("accepted")?;
    assert!(matches!(
        over,
        GenerateError::JobAuthority(JobAuthorityError::TooManyWorkflows {
            listed: 33,
            limit: 32
        })
    ));
    let at = expected_with_notes(&listed(32)?).err().ok_or("accepted")?;
    assert!(matches!(
        at,
        GenerateError::JobAuthority(JobAuthorityError::NotGenerated(_))
    ));
    Ok(())
}

fn jobs_listed(count: usize) -> Result<RepoView, Box<dyn Error>> {
    let mut workflows = "[[workflow]]\nfile = \"gated\"\nname = \"Gated\"\n\n[workflow.on.push]\nbranches = [\"main\"]\n".to_owned();
    for index in 0..count {
        write!(
            workflows,
            "\n[[workflow.job]]\nid = \"j{index:04}\"\npermissions = {{ contents = \"read\" }}\n\n[[workflow.job.step]]\nname = \"Run\"\nrun = \"echo ok\"\n"
        )?;
    }
    enabled("full_run = \"off\"\n", Some(&workflows), &[GATED])
}

#[test]
fn more_jobs_than_the_host_accepts_are_refused_and_the_limit_itself_is_not() -> TestResult {
    let limit = velnor_model::JOB_AUTHORITY_MAX_JOBS;
    let over = expected_with_notes(&jobs_listed(limit + 1)?)
        .err()
        .ok_or("accepted")?;
    assert!(matches!(
        over,
        GenerateError::JobAuthority(JobAuthorityError::TooManyJobs { listed, limit: max })
            if listed == limit + 1 && max == limit
    ));
    let (files, _) = expected_with_notes(&jobs_listed(limit)?)?;
    assert!(files.contains_key(&JOB_AUTHORITY_INVENTORY_PATH));
    Ok(())
}

fn tracked(fixture: &Fixture) -> Result<BTreeSet<RepoPath>, Box<dyn Error>> {
    let output = std::process::Command::new(crate::setup_action::program("git"))
        .args(["ls-files"])
        .current_dir(fixture.repo())
        .output()?;
    text(&output.stdout)
        .lines()
        .map(|line| Ok(RepoPath::new(line)?))
        .collect()
}

#[test]
fn check_of_an_unstamped_build_passes_on_a_correct_tree() -> TestResult {
    let fixture = fixture()?;
    fixture.write(
        ".velnor/job-authority.toml",
        "workflows = [\".github/workflows/ci.yml\"]\n",
    )?;
    git(&fixture.repo(), &["add", "--all"])?;
    let cargo = fixture.base.join("bin/cargo-in-process");
    std::fs::write(
        &cargo,
        format!(
            "#!/bin/sh\n/bin/cat {}\n",
            fixture.base.join("bin/metadata.json").display()
        ),
    )?;
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755))?;
    let (view, _) = view_of(
        &fixture.repo(),
        tracked(&fixture)?,
        BTreeSet::new(),
        BTreeSet::new(),
        false,
        Some(&velnor_gen::io::Cargo {
            executable: &cargo.display().to_string(),
            toolchain: None,
        }),
    )?;
    let files = generated_with_notes(&view)?.0;
    assert!(files.contains_key(&JOB_AUTHORITY_INVENTORY_PATH));
    std::fs::create_dir_all(fixture.repo().join(".github/workflows"))?;
    for (path, content) in &files {
        fixture.write(path.as_str(), content)?;
    }
    git(&fixture.repo(), &["add", "--all"])?;
    let output = fixture.velnor_gen("check")?;
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    Ok(())
}
