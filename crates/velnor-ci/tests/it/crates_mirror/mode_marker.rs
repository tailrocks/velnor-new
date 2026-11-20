// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::io::Repo;

use super::fixture::{TestResult, setup, setup_command};
use super::mbx::{
    MBX_DECLARED, REGISTRY, configured_repo, path_with, plain_cargo, wrapper_command,
};

const MARKER_FILE: &str = "velnor-cargo/velnor-cargo-mode";
const MARKER_CONFIG: &str = "recorded";

fn run_with_step_env(
    repo: &Repo,
    mbx_required: bool,
    step_env: &[(&str, &str)],
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let plain = plain_cargo(repo)?;
    let mut command = wrapper_command(repo.root(), &plain, mbx_required, &[])?;
    command.envs(step_env.iter().copied());
    Ok(command.output()?)
}

fn assert_refused(output: &std::process::Output, message: &str) {
    let stderr = String::from_utf8_lossy(output.stderr.as_slice());
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains(message), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
}

#[test]
fn step_env_cannot_turn_the_mirror_off_in_a_mirror_on_job() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-off-and-empty")?;
    let output = run_with_step_env(
        &repo,
        false,
        &[("VELNOR_CRATES_MIRROR", "off"), ("VELNOR_CARGO_CONFIG", "")],
    )?;
    assert_refused(&output, "VELNOR_CRATES_MIRROR=off");
    Ok(())
}

#[test]
fn step_env_cannot_blank_the_mirror_config_in_a_mirror_on_job() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-empty")?;
    let output = run_with_step_env(&repo, false, &[("VELNOR_CARGO_CONFIG", "")])?;
    assert_refused(&output, "VELNOR_CARGO_CONFIG");
    Ok(())
}

#[test]
fn step_env_cannot_switch_the_mirror_off_while_keeping_the_config() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-off-only")?;
    let output = run_with_step_env(&repo, false, &[("VELNOR_CRATES_MIRROR", "off")])?;
    assert_refused(&output, "VELNOR_CRATES_MIRROR=off");
    Ok(())
}

#[test]
fn step_env_cannot_drop_a_declared_mbx_requirement() -> TestResult {
    let repo = Repo::empty("crates-mirror-marker-mbx");
    repo.write_unstaged("ca.pem", "not used by this fixture");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let output = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("PATH", path_with(repo.root())?)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let output = run_with_step_env(&repo, false, &[("VELNOR_CARGO_MBX", "")])?;
    assert_refused(&output, super::mbx::REFUSED);
    Ok(())
}

#[test]
fn a_deleted_mode_record_cannot_swap_the_mirror_config() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-deleted")?;
    fs::remove_file(repo.root().join(MARKER_FILE))?;
    let evil = repo.root().join("evil.toml");
    fs::write(&evil, "[source.crates-io]\n")?;
    let output = run_with_step_env(
        &repo,
        false,
        &[("VELNOR_CARGO_CONFIG", &evil.display().to_string())],
    )?;
    assert_refused(&output, "mode record");
    Ok(())
}

#[test]
fn a_deleted_mode_record_cannot_drop_a_declared_mbx_requirement() -> TestResult {
    let repo = Repo::empty("crates-mirror-marker-deleted-mbx");
    repo.write_unstaged("ca.pem", "not used by this fixture");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let output = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("PATH", path_with(repo.root())?)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    fs::remove_file(repo.root().join(MARKER_FILE))?;
    let output = run_with_step_env(&repo, false, &[("VELNOR_CARGO_MBX", "")])?;
    assert_refused(&output, "mode record");
    Ok(())
}

#[test]
fn setup_records_the_mirror_mode_beside_the_wrapper() -> TestResult {
    let on = configured_repo("crates-mirror-marker-record-on")?;
    assert!(read_marker(on.root())?.contains("mirror=on"));
    let off = Repo::empty("crates-mirror-marker-record-off");
    off.write_unstaged("mise.toml", MBX_DECLARED);
    let output = setup_command(off.root(), REGISTRY)
        .current_dir(off.root())
        .env("VELNOR_CRATES_MIRROR", "off")
        .env("PATH", path_with(off.root())?)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let text = read_marker(off.root())?;
    assert!(
        text.contains("mirror=off") && text.contains("mbx=required"),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_later_mirror_on_setup_supersedes_an_earlier_mirror_off_marker() -> TestResult {
    let repo = Repo::empty("crates-mirror-marker-rerun");
    repo.write_unstaged("ca.pem", "not used by this fixture");
    repo.write_unstaged("mise.toml", MBX_DECLARED);
    let off = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("VELNOR_CRATES_MIRROR", "off")
        .env("PATH", path_with(repo.root())?)
        .output()?;
    assert!(off.status.success(), "{off:?}");
    let on = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .env("PATH", path_with(repo.root())?)
        .output()?;
    assert!(on.status.success(), "{on:?}");
    assert!(read_marker(repo.root())?.contains("mirror=on"));
    Ok(())
}

#[test]
fn step_env_cannot_replace_the_mirror_config_with_another_file() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-other-config")?;
    let evil = repo.root().join("evil.toml");
    fs::write(&evil, "[source.crates-io]\n")?;
    let output = run_with_step_env(
        &repo,
        false,
        &[("VELNOR_CARGO_CONFIG", &evil.display().to_string())],
    )?;
    assert_refused(&output, "VELNOR_CARGO_CONFIG");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(MARKER_CONFIG),
        "{output:?}"
    );
    Ok(())
}

#[test]
fn step_env_naming_the_recorded_mirror_config_still_runs_cargo() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-same-config")?;
    let config = fs::canonicalize(repo.root().join("velnor-cargo/config.toml"))?;
    let output = run_with_step_env(
        &repo,
        false,
        &[("VELNOR_CARGO_CONFIG", &config.display().to_string())],
    )?;
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("plain-cargo-ran"), "{stdout}");
    Ok(())
}

#[test]
fn setup_records_the_config_path_it_wrote() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-config-line")?;
    let config = fs::canonicalize(repo.root().join("velnor-cargo/config.toml"))?;
    assert!(
        read_marker(repo.root())?.contains(&format!("config={}", config.display())),
        "{}",
        read_marker(repo.root())?
    );
    Ok(())
}

#[test]
fn setup_does_not_write_through_a_planted_record_symlink() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-symlink")?;
    let victim = repo.root().join("victim");
    fs::write(&victim, "precious")?;
    std::os::unix::fs::symlink(&victim, repo.root().join(format!("{MARKER_FILE}.tmp")))?;
    let output = setup(repo.root(), REGISTRY);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&victim)?, "precious");
    assert!(read_marker(repo.root())?.contains("mirror=on"));
    Ok(())
}

#[test]
fn setup_replaces_a_stale_regular_record_temp_file() -> TestResult {
    let repo = configured_repo("crates-mirror-marker-stale-temp")?;
    fs::write(repo.root().join(format!("{MARKER_FILE}.tmp")), "stale")?;
    let output = setup(repo.root(), REGISTRY);
    assert!(output.status.success(), "{output:?}");
    assert!(read_marker(repo.root())?.contains("mirror=on"));
    Ok(())
}

fn read_marker(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(root.join(MARKER_FILE))?)
}
