// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeSet, MAX_ARTIFACT_BYTES, Path, PathBuf, PreparationArtifact, PreparationBinding,
    PreparationCheckout, fs, read_regular, sha256,
};

pub(super) fn execute_preparation_command(
    checkout: PreparationCheckout,
    command: &velnor_actions_mise::command::IsolatedCommand,
    timeout: std::time::Duration,
) -> Result<PreparationArtifact, &'static str> {
    let deadline = velnor_actions_mise::CheckDeadline::after(timeout)
        .map_err(|_| "release_pr_update_failed")?;
    let (output, _drained) = command
        .run_quiescent_until(1024 * 1024, deadline)
        .map_err(|_| "release_pr_update_failed")?;
    if !output.success {
        return Err("release_pr_update_failed");
    }
    checkout.collect()
}

#[cfg(test)]
pub(crate) fn fixture_execute_preparation_command(
    checkout: PreparationCheckout,
    command: &velnor_actions_mise::command::IsolatedCommand,
) -> Result<PreparationArtifact, &'static str> {
    execute_preparation_command(checkout, command, std::time::Duration::from_secs(10))
}

pub fn prepare_pr_from_environment() -> Result<(), crate::OrchestratorError> {
    prepare_pr_inner().map_err(crate::internal::internal)
}

fn prepare_pr_inner() -> Result<(), &'static str> {
    use velnor_actions_workflow_renderer::release_config::{
        ReleasePlzConfig, ReleasePlzPackage, render_release_plz_config,
    };
    let invalid = || "release_pr_preparation_invalid";
    let required = |key: &str| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid)
    };
    let source = PathBuf::from(required("GITHUB_WORKSPACE")?)
        .canonicalize()
        .map_err(|_| invalid())?;
    let temp = PathBuf::from(required("RUNNER_TEMP")?)
        .canonicalize()
        .map_err(|_| invalid())?;
    if temp.starts_with(&source) || !temp.is_dir() {
        return Err(invalid());
    }
    let intent_bytes = read_regular(&source, ".github/velnor-release-intent.toml", 1024 * 1024)?;
    let intent = crate::parse_crate_release_intent(&intent_bytes).map_err(|_| invalid())?;
    let source_sha = required("GITHUB_SHA")?;
    let run_id = required("GITHUB_RUN_ID")?
        .parse::<u64>()
        .map_err(|_| invalid())?;
    let run_attempt = required("GITHUB_RUN_ATTEMPT")?
        .parse::<u32>()
        .map_err(|_| invalid())?;
    if required("GITHUB_EVENT_NAME")? != "push"
        || required("GITHUB_REPOSITORY")? != intent.repository
        || required("GITHUB_REF")? != format!("refs/heads/{}", intent.default_branch)
        || run_attempt != 1
    {
        return Err(invalid());
    }
    let binding = PreparationBinding {
        repository: intent.repository.clone(),
        workflow_sha: source_sha.clone(),
        source_sha,
        run_id,
        run_attempt,
    };
    let allowed: BTreeSet<String> = intent.pr_write_paths.iter().cloned().collect();
    let config = ReleasePlzConfig {
        tag_pattern: intent.tag_template.clone(),
        semver_check: true,
        packages: intent
            .packages
            .iter()
            .map(|name| ReleasePlzPackage {
                name: name.clone(),
                publish_features: intent.publish_features[name].clone(),
            })
            .collect(),
    };
    let expected =
        render_release_plz_config(&config, &intent.generator_version).map_err(|_| invalid())?;
    if read_regular(&source, ".github/release-plz.toml", 1024 * 1024)? != expected.as_bytes() {
        return Err("release_pr_generated_config_mismatch");
    }
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&temp.join("velnor/git-bundle"))
            .map_err(|_| invalid())?,
    );
    let checkout = PreparationCheckout::open(&source, binding, allowed, git)?;
    let artifact = run_preparation_update(checkout, &intent, &source, &temp)?;
    let output = PathBuf::from(required("GITHUB_OUTPUT")?);
    write_preparation_artifact(&artifact, &temp, &output)
}

fn write_preparation_artifact(
    artifact: &PreparationArtifact,
    temp: &Path,
    output: &Path,
) -> Result<(), &'static str> {
    use std::io::Write as _;
    let invalid = || "release_pr_preparation_invalid";
    let bytes = serde_json::to_vec(&artifact).map_err(|_| invalid())?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(invalid());
    }
    let artifact_path = temp.join("velnor-release-pr-preparation.json");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(artifact_path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|_| invalid())?;
    if !output.is_absolute() {
        return Err(invalid());
    }
    let mut output = fs::OpenOptions::new()
        .append(true)
        .open(output)
        .map_err(|_| invalid())?;
    writeln!(output, "content_sha256={}", sha256(&bytes)).map_err(|_| invalid())?;
    writeln!(output, "changed={}", !artifact.files.is_empty()).map_err(|_| invalid())?;
    Ok(())
}

fn run_preparation_update(
    checkout: PreparationCheckout,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    source: &Path,
    temp: &Path,
) -> Result<PreparationArtifact, &'static str> {
    let invalid = || "release_pr_preparation_invalid";
    let git_root = temp.join("velnor/git-bundle");
    let git =
        velnor_actions_mise::release_git_pin::verify_bundle(&git_root).map_err(|_| invalid())?;
    let plz = temp.join("velnor-release-plz/release-plz");
    let semver = temp.join("velnor-semver/cargo-semver-checks");
    let plz_pin = velnor_actions_mise::catalog::qualified_release_plz(
        &intent.release_plz_version,
        &intent.runner,
    )
    .ok_or_else(invalid)?;
    let semver_pin = velnor_actions_mise::catalog::qualified_cargo_semver_checks(&intent.runner)
        .ok_or_else(invalid)?;
    check_pinned_executable(&plz, plz_pin.executable_sha256)?;
    check_pinned_executable(&semver, semver_pin.executable_sha256)?;
    let rustup = temp.join("velnor/rustup");
    let cargo = crate::crate_release_source::verified_rust_tool_path(
        &rustup,
        &intent.rust_version,
        "cargo",
    )
    .map_err(|_| invalid())?;
    let rustc = crate::crate_release_source::verified_rust_tool_path(
        &rustup,
        &intent.rust_version,
        "rustc",
    )
    .map_err(|_| invalid())?;
    let rustdoc = crate::crate_release_source::verified_rust_tool_path(
        &rustup,
        &intent.rust_version,
        "rustdoc",
    )
    .map_err(|_| invalid())?;
    let rust_hashes: Vec<_> = [&cargo, &rustc, &rustdoc]
        .iter()
        .map(|path| {
            fs::read(path)
                .map(|bytes| sha256(&bytes))
                .map_err(|_| invalid())
        })
        .collect::<Result<_, _>>()?;
    let scratch = tempfile::Builder::new()
        .prefix("velnor-pr-")
        .tempdir_in(temp)
        .map_err(|_| invalid())?;
    let tools = PreparationToolPaths {
        cargo: &cargo,
        rustc: &rustc,
        rustdoc: &rustdoc,
        rustup: &rustup,
        semver: &semver,
        temp,
    };
    let env = preparation_tool_environment(&tools, scratch.path())?;
    let args = vec![
        "update".into(),
        "--config".into(),
        source.join(".github/release-plz.toml").into_os_string(),
        "--manifest-path".into(),
        source.join(&intent.manifest_path).into_os_string(),
    ];
    let command =
        velnor_actions_mise::command::IsolatedCommand::crate_update(&plz, args, source, &env, &git)
            .map_err(|_| invalid())?;
    velnor_actions_mise::release_git_pin::verify_bundle(&git_root).map_err(|_| invalid())?;
    let result =
        execute_preparation_command(checkout, &command, std::time::Duration::from_mins(20));
    velnor_actions_mise::release_git_pin::verify_bundle(&git_root).map_err(|_| invalid())?;
    let artifact = result?;
    check_pinned_executable(&plz, plz_pin.executable_sha256)?;
    check_pinned_executable(&semver, semver_pin.executable_sha256)?;
    for (path, expected) in [&cargo, &rustc, &rustdoc].iter().zip(rust_hashes) {
        if sha256(&fs::read(path).map_err(|_| invalid())?) != expected {
            return Err(invalid());
        }
    }
    Ok(artifact)
}

struct PreparationToolPaths<'a> {
    cargo: &'a Path,
    rustc: &'a Path,
    rustdoc: &'a Path,
    rustup: &'a Path,
    semver: &'a Path,
    temp: &'a Path,
}

fn preparation_tool_environment(
    tools: &PreparationToolPaths<'_>,
    scratch: &Path,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, &'static str> {
    use std::ffi::OsString;
    let invalid = || "release_pr_preparation_invalid";
    let home = scratch.join("home");
    let cargo_home = scratch.join("cargo");
    let target = scratch.join("target");
    for dir in [&home, &cargo_home, &target] {
        fs::create_dir(dir).map_err(|_| invalid())?;
    }
    let path = std::env::join_paths([
        tools.semver.parent().ok_or_else(invalid)?,
        tools.cargo.parent().ok_or_else(invalid)?,
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .map_err(|_| invalid())?;
    let env: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), path),
        ("CARGO".into(), tools.cargo.as_os_str().into()),
        ("RUSTC".into(), tools.rustc.as_os_str().into()),
        ("RUSTDOC".into(), tools.rustdoc.as_os_str().into()),
        ("HOME".into(), home.as_os_str().into()),
        ("CARGO_HOME".into(), cargo_home.as_os_str().into()),
        ("CARGO_TARGET_DIR".into(), target.as_os_str().into()),
        ("RUSTUP_HOME".into(), tools.rustup.as_os_str().into()),
        ("RUSTUP_AUTO_INSTALL".into(), "0".into()),
        ("RUNNER_TEMP".into(), tools.temp.as_os_str().into()),
    ];
    Ok(env)
}

pub(super) fn check_pinned_executable(path: &Path, expected: &str) -> Result<(), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "release_pr_tool_invalid")?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 256 * 1024 * 1024
    {
        return Err("release_pr_tool_invalid");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("release_pr_tool_invalid");
        }
    }
    if sha256(&fs::read(path).map_err(|_| "release_pr_tool_invalid")?) != expected {
        return Err("release_pr_tool_invalid");
    }
    Ok(())
}
