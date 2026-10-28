// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::PathBuf};

use serde_json::{Value, json};

use super::{CapturedOutcome, FixtureProfile, FixtureRuntime, ReplayMutation, manifest_outcomes};

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
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));
    let examples = cli
        .parent()
        .ok_or("CLI executable has no parent directory")?
        .join("examples");
    let executable = examples.join(format!(
        "image-publication-fixture{}",
        std::env::consts::EXE_SUFFIX
    ));
    if !executable.is_file() {
        return Err(format!("registered image-publication-fixture is missing at {}; run package nextest so Cargo builds examples", executable.display()).into());
    }
    Ok(executable)
}
