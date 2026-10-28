// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error, fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command, process::Stdio,
};

use serde_json::{Value, json};

use super::{
    CapturedOutcome, Fixture, FixtureProfile, FixtureRuntime, ReplayMutation, manifest_outcomes,
};

pub(super) fn publication_command(fixture: &Fixture) -> Result<Command, std::io::Error> {
    let path = fixture
        .mise_bin
        .parent()
        .ok_or_else(|| std::io::Error::other("fixture mise binary has no parent directory"))?;
    let home = fixture
        .state
        .parent()
        .ok_or_else(|| std::io::Error::other("fixture state has no parent directory"))?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PATH", path)
        .env("HOME", home)
        .env("GH_TOKEN", "fixture-source-token")
        .env("DOCKER_CONFIG", &fixture.docker_config)
        .args([
            "image-maintenance",
            "publish-image",
            "--profile",
            fixture.profile.cli_name(),
            "--source-commit",
        ])
        .arg(&fixture.source)
        .args(["--producer-run-id", &fixture.producer_run_id.to_string()])
        .args([
            "--producer-run-attempt",
            &fixture.producer_run_attempt.to_string(),
        ])
        .args([
            "--artifact-id",
            &fixture.artifact_id.to_string(),
            "--archive-path",
        ])
        .arg(&fixture.archive)
        .arg("--record-path")
        .arg(&fixture.record)
        .arg("--private-scratch")
        .arg(&fixture.scratch)
        .arg("--receipt-output")
        .arg(&fixture.receipt);
    Ok(command)
}

pub(super) fn create_runtime(
    base: &std::path::Path,
    mut state_value: Value,
    profile: FixtureProfile,
    source: &str,
) -> Result<FixtureRuntime, Box<dyn Error>> {
    let home = base.join("home");
    fs::create_dir(&home)?;
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
    let state = home.join("fixture-state.json");
    state_value["crane"]["tag_lookup_output"] = manifest_outcomes::replay(
        CapturedOutcome::Missing,
        ReplayMutation::Exact,
        profile,
        source,
    )?;
    state_value["crane"]["tag_lookup_output_consumed"] = json!(false);
    fs::write(&state, serde_json::to_vec(&state_value)?)?;
    let scratch = base.join("scratch");
    fs::create_dir(&scratch)?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
    let docker_config = base.join("docker-config");
    fs::create_dir(&docker_config)?;
    fs::set_permissions(&docker_config, fs::Permissions::from_mode(0o700))?;
    let config = docker_config.join("config.json");
    fs::write(&config, b"{}")?;
    fs::set_permissions(config, fs::Permissions::from_mode(0o600))?;
    let bin = base.join("bin");
    fs::create_dir(&bin)?;
    let mise_bin = bin.join("mise");
    fs::copy(fixture_executable()?, &mise_bin)?;
    fs::set_permissions(&mise_bin, fs::Permissions::from_mode(0o755))?;
    Ok(FixtureRuntime {
        state,
        scratch,
        docker_config,
        mise_bin,
        error_path: home.join("image-publication-fixture-error"),
    })
}

fn fixture_executable() -> Result<PathBuf, Box<dyn Error>> {
    let executable = std::env::var_os("VELNOR_IMAGE_PUBLICATION_FIXTURE")
        .map(PathBuf::from)
        .ok_or("VELNOR_IMAGE_PUBLICATION_FIXTURE is required")?;
    if !executable.is_absolute() {
        return Err("VELNOR_IMAGE_PUBLICATION_FIXTURE must be absolute".into());
    }
    let metadata = fs::symlink_metadata(&executable)?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "VELNOR_IMAGE_PUBLICATION_FIXTURE is not an executable file: {}",
            executable.display()
        )
        .into());
    }
    Ok(executable)
}
