// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn verify_checker(
    receipt: &CrateReleaseAdmissionReceipt,
    runner_temp: &Path,
) -> Result<std::path::PathBuf, OrchestratorError> {
    let qualified =
        velnor_actions_mise::catalog::qualified_cargo_semver_checks(&receipt.intent.runner)
            .ok_or_else(invalid)?;
    let relative = "velnor-semver/cargo-semver-checks";
    let binary = match crate::safe_read::read_repo_bytes(runner_temp, relative, 128 * 1024 * 1024)?
    {
        crate::safe_read::RepoBytes::Bytes(bytes) if !bytes.is_empty() => bytes,
        _ => return Err(invalid()),
    };
    if crate::generator::sha256_hex(&binary) != qualified.executable_sha256 {
        return Err(invalid());
    }
    Ok(runner_temp.join(relative))
}

pub(crate) fn verify_one<R: CrateRegistryRead>(
    source: &Path,
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    package: &crate::crate_release_controller::CrateReleasePlannedPackage,
    executable: &Path,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<SemverEvidence, OrchestratorError> {
    let checker_sha256 =
        velnor_actions_mise::catalog::qualified_cargo_semver_checks(&receipt.intent.runner)
            .ok_or_else(invalid)?
            .executable_sha256
            .to_owned();
    let Some(previous) = baseline(registry, &package.source.name, &package.source.version)? else {
        let approved_new = matches!(
            package.owners,
            crate::crate_release_registry::CrateRegistryOwnership::NewCrate
        ) && matches!(
            package.action,
            crate::crate_release_controller::CrateReleaseAction::Publish
        ) && receipt.github.request.event == "workflow_dispatch"
            && receipt.intent.authentication
                == velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken
            && receipt.intent.new_crates.as_ref().is_some_and(|creation| {
                creation.source_sha == receipt.github.request.source_sha
                    && creation.packages.get(&package.source.name) == Some(&package.source.version)
            });
        if !approved_new {
            return Err(invalid());
        }
        return Ok(SemverEvidence::NoBaseline { checker_sha256 });
    };
    let features = receipt
        .intent
        .publish_features
        .get(&package.source.name)
        .ok_or_else(invalid)?;
    let (archive, baseline_checksum) =
        download_baseline(registry, &package.source.name, &previous)?;
    source_execution.check_semver(&crate::CrateSemverSource {
        source,
        rust: &receipt.intent.rust_version,
        manifest: &package.source.manifest,
        package: &package.source.name,
        baseline_version: &previous.version,
        baseline_archive: &archive,
        baseline_checksum: &baseline_checksum,
        features,
        executable,
        git,
    })?;
    Ok(SemverEvidence::Checked {
        baseline: previous.version,
        baseline_checksum,
        checker_sha256,
    })
}

pub(super) fn download_baseline<R: CrateRegistryRead>(
    registry: &R,
    package: &str,
    baseline: &Baseline,
) -> Result<(Vec<u8>, String), OrchestratorError> {
    let version = baseline.version.replace('+', "%2B");
    let response = registry.get(&format!("/api/v1/crates/{package}/{version}"))?;
    if response.status != 200 || response.body.len() > 8 * 1024 * 1024 {
        return Err(invalid());
    }
    let reply: BaselineReply = serde_json::from_slice(&response.body).map_err(|_| invalid())?;
    let item = reply.version;
    let checksum = &item.checksum;
    if checksum.len() != 64
        || !checksum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || item.id != baseline.id
        || item.package != package
        || item.num != baseline.version
    {
        return Err(invalid());
    }
    let archive = registry.get(&format!("/crates/{package}/{package}-{version}.crate"))?;
    if archive.status != 200
        || archive.body.len() > 64 * 1024 * 1024
        || crate::generator::sha256_hex(&archive.body) != *checksum
    {
        return Err(invalid());
    }
    Ok((archive.body, item.checksum))
}

pub(super) fn extract_baseline(
    bytes: &[u8],
    package: &str,
    version: &str,
    destination: &Path,
) -> Result<(), OrchestratorError> {
    let files =
        crate::crate_release_archive::read_archive(bytes, &format!("{package}-{version}/"))?;
    let manifest = files.get("Cargo.toml").ok_or_else(invalid)?;
    let manifest: toml::Value =
        toml::from_str(std::str::from_utf8(&manifest.1).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    if manifest["package"]["name"].as_str() != Some(package)
        || manifest["package"]["version"].as_str() != Some(version)
    {
        return Err(invalid());
    }
    for (path, (mode, bytes)) in files {
        let output = destination.join(path);
        std::fs::create_dir_all(output.parent().ok_or_else(invalid)?).map_err(|_| invalid())?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|_| invalid())?;
        file.write_all(&bytes).map_err(|_| invalid())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(0o600 | mode))
                .map_err(|_| invalid())?;
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn run_check(
    source: &Path,
    rust: &str,
    manifest: &str,
    package: &str,
    baseline: (&str, &[u8]),
    tools: (&[String], &Path),
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let (features, executable) = tools;
    let temp = tempfile::tempdir().map_err(|_| invalid())?;
    let home = temp.path().join("cargo-home");
    std::fs::create_dir(&home).map_err(|_| invalid())?;
    #[cfg(all(test, unix))]
    let fixture_registry =
        if let Some(snapshot) = std::env::var_os("VELNOR_NATIVE_REGISTRY_SNAPSHOT") {
            test_registry::copy_snapshot(Path::new(&snapshot), &home).map_err(|_| invalid())?;
            true
        } else {
            false
        };
    let baseline_root = temp.path().join("baseline");
    std::fs::create_dir(&baseline_root).map_err(|_| invalid())?;
    extract_baseline(baseline.1, package, baseline.0, &baseline_root)?;
    let cargo = source_cargo_path(rust, temp.path())?;
    let mut args = vec![
        OsString::from("-i"),
        format!("HOME={}", temp.path().display()).into(),
        format!("CARGO_HOME={}", home.display()).into(),
        format!("CARGO_TARGET_DIR={}", temp.path().join("target").display()).into(),
        format!("CARGO={}", cargo.display()).into(),
    ];
    args.extend(source_compiler_env(rust, git)?);
    #[cfg(all(test, unix))]
    if fixture_registry {
        args.push("CARGO_NET_OFFLINE=true".into());
    }
    args.extend([
        executable.as_os_str().to_owned(),
        "check-release".into(),
        "--manifest-path".into(),
        source.join(manifest).into_os_string(),
        "--package".into(),
        package.into(),
        "--baseline-root".into(),
        baseline_root.into_os_string(),
        "--default-features".into(),
    ]);
    if !features.is_empty() {
        args.extend(["--current-features".into(), features.join(",").into()]);
    }
    let command = PinnedToolExec::new(vec![PinnedTool::Rust], std::ffi::OsStr::new("env"), args)
        .map_err(|_| invalid())?
        .command(&source_tool_catalog(rust).map_err(|_| invalid())?)
        .map_err(|_| invalid())?
        .with_cwd(source.to_path_buf());
    let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(600))
        .map_err(|_| invalid())?;
    let (output, _) = command
        .run_quiescent_until(1024 * 1024, deadline)
        .map_err(|_| invalid())?;
    if !output.success {
        #[cfg(test)]
        {
            let text = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
            for (name, pattern) in [
                ("registry-unavailable", "failed to download"),
                ("registry-unavailable", "failed to get"),
                ("registry-unavailable", "failed to fetch"),
                ("dependency-unavailable", "no matching package named"),
                ("network-unavailable", "failed to resolve"),
                ("network-unavailable", "could not resolve"),
                ("manifest-error", "failed to parse manifest"),
                ("rustdoc-error", "failed to generate rustdoc"),
                ("rustdoc-error", "failed to build"),
                ("semver-violations", "semver requires"),
                ("semver-violations", "breaking change"),
            ] {
                if text.contains(pattern) {
                    eprintln!("fixture semver class: {name}");
                }
            }
            eprintln!("fixture semver checker exited unsuccessfully");
        }
        return Err(invalid());
    }
    velnor_actions_mise::release_git_pin::ReleaseGitExecutable::revalidate(git)
        .map_err(|_| invalid())?;
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy)]
pub(crate) struct ProjectionInput<'a> {
    pub archive: &'a [u8],
    pub checksum: &'a str,
    pub package: &'a str,
    pub baseline_version: &'a str,
    pub source_sha: &'a str,
    pub target_version: &'a str,
    pub manifest: &'a str,
    pub features: &'a [String],
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn supervised_projection(
    request: &ProjectionInput<'_>,
) -> Result<
    (
        serde_json::Value,
        crate::crate_release_archive::ArchiveFiles,
    ),
    OrchestratorError,
> {
    if crate::generator::sha256_hex(request.archive) != request.checksum {
        return Err(invalid());
    }
    let files = crate::crate_release_archive::read_archive(
        request.archive,
        &format!("{}-{}/", request.package, request.baseline_version),
    )?;
    let manifest = files.get("Cargo.toml").ok_or_else(invalid)?;
    let manifest: toml::Value =
        toml::from_str(std::str::from_utf8(&manifest.1).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    if manifest["package"]["name"].as_str() != Some(request.package)
        || manifest["package"]["version"].as_str() != Some(request.baseline_version)
        || !files.contains_key("Cargo.lock")
    {
        return Err(invalid());
    }
    let inventory: Vec<_> = files
        .iter()
        .map(|(path, (mode, bytes))| {
            serde_json::json!({
                "path": path,
                "sha256": crate::generator::sha256_hex(bytes),
                "kind": {"type": "regular", "executable": *mode != 0},
            })
        })
        .collect();
    let context = serde_json::json!({
        "source_sha": request.source_sha,
        "package": request.package,
        "target_version": request.target_version,
        "manifest": request.manifest,
        "features": request.features,
        "baseline_version": request.baseline_version,
        "baseline_checksum": request.checksum,
        "files": inventory,
    });
    Ok((context, files))
}
