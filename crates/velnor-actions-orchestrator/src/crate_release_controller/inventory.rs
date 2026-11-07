// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
use super::tests;
use super::{
    CrateRegistryOwner, CrateRegistryRead, CrateReleaseIntent, CrateReleasePlannedPackage,
    CrateReleaseSourcePackage, GitHubAdmissionRead, OrchestratorError, Path,
    crate_release_workspace_plan_with_registry, internal, read_github_tag_evidence,
};

pub(super) fn workspace_inventory<R: CrateRegistryRead>(
    source: &Path,
    source_sha: &str,
    intent: &CrateReleaseIntent,
    registry: &R,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<Vec<CrateReleaseSourcePackage>, OrchestratorError> {
    let workspace = crate_release_workspace_plan_with_registry(
        source,
        source_sha,
        intent,
        registry,
        git,
        source_execution,
    )?;
    let plan = workspace.plan;
    plan.graph
        .order
        .iter()
        .map(|name| {
            let package = plan
                .selection
                .packages
                .iter()
                .find(|package| &package.name == name)
                .ok_or_else(|| internal("crate_release_workspace_plan_invalid"))?;
            Ok(CrateReleaseSourcePackage {
                name: package.name.clone(),
                version: package.version.clone(),
                manifest: package.manifest.clone(),
            })
        })
        .collect()
}

pub(crate) fn package_unprivileged(
    source: &Path,
    intent: &CrateReleaseIntent,
    package: &CrateReleaseSourcePackage,
    target: &Path,
    expected_sha: &str,
    verify: bool,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<u8>, OrchestratorError> {
    use std::ffi::{OsStr, OsString};
    use velnor_actions_mise::{PinnedTool, PinnedToolExec};
    crate::verify_crate_release_source(source, expected_sha, git)?;
    crate::release::crate_release_source::verify_toolchain_source_separation(source)?;
    let mut args = vec![
        OsString::from("package"),
        OsString::from("--locked"),
        OsString::from("--manifest-path"),
        source.join(&intent.manifest_path).into_os_string(),
        OsString::from("--package"),
        OsString::from(&package.name),
        OsString::from("--target-dir"),
        target.as_os_str().to_owned(),
    ];
    let features = intent
        .publish_features
        .get(&package.name)
        .ok_or_else(|| internal("crate_release_package_features_missing"))?;
    if !features.is_empty() {
        args.push(OsString::from("--features"));
        args.push(OsString::from(features.join(",")));
    }
    if !verify {
        args.push(OsString::from("--no-verify"));
    }
    let sterile = tempfile::tempdir().map_err(|_| internal("crate_release_package_temp"))?;
    let cargo_home = sterile.path().join("cargo-home");
    std::fs::create_dir(&cargo_home).map_err(|_| internal("crate_release_package_temp"))?;
    #[cfg(test)]
    tests::configure_test_registry(&cargo_home)?;
    #[cfg(all(test, unix))]
    let fixture_registry = seed_package_fixture(&cargo_home)?;
    let catalog = crate::release::crate_release_source::source_tool_catalog(&intent.rust_version)
        .map_err(|_| internal("crate_release_toolchain_invalid"))?;
    let mut isolated_args = vec![
        OsString::from("-i"),
        OsString::from(format!("HOME={}", sterile.path().display())),
        OsString::from(format!("CARGO_HOME={}", cargo_home.display())),
    ];
    #[cfg(all(test, unix))]
    if fixture_registry {
        isolated_args.push(OsString::from("CARGO_NET_OFFLINE=true"));
    }
    isolated_args.extend(crate::release::crate_release_source::source_compiler_env(
        &intent.rust_version,
        git,
    )?);
    isolated_args.push(
        crate::release::crate_release_source::source_cargo_path(
            &intent.rust_version,
            sterile.path(),
        )?
        .into_os_string(),
    );
    isolated_args.extend(args);
    let exec = PinnedToolExec::new(vec![PinnedTool::Rust], OsStr::new("env"), isolated_args)
        .map_err(|_| internal("crate_release_package_command"))?;
    let result = exec
        .command(&catalog)
        .map_err(|_| internal("crate_release_package_command"))?
        .with_cwd(if verify {
            source.to_path_buf()
        } else {
            sterile.path().to_path_buf()
        })
        .run()
        .map_err(|_| internal("crate_release_package_failed"))?;
    if !result.success {
        return Err(internal("crate_release_package_failed"));
    }
    crate::verify_crate_release_source(source, expected_sha, git)?;
    let relative = format!("package/{}-{}.crate", package.name, package.version);
    match crate::safe_read::read_repo_bytes(target, &relative, 64 * 1024 * 1024)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => Ok(bytes),
        crate::safe_read::RepoBytes::Absent => Err(internal("crate_release_package_missing")),
    }
}

pub(crate) fn list_package_unprivileged(
    request: &crate::CratePackageSource<'_>,
    deadline: velnor_actions_mise::CheckDeadline,
    home: &Path,
    prime_index: bool,
) -> Result<Vec<String>, OrchestratorError> {
    use std::ffi::OsString;
    let invalid = || internal("crate_release_package_list_invalid");
    let cargo_home = home.join("cargo-home");
    #[cfg(test)]
    if prime_index {
        tests::configure_test_registry(&cargo_home)?;
        #[cfg(unix)]
        let _ = seed_package_fixture(&cargo_home)?;
    }
    let mut args = vec![
        OsString::from("-i"),
        format!("HOME={}", home.display()).into(),
        format!("CARGO_HOME={}", cargo_home.display()).into(),
    ];
    args.extend(crate::release::crate_release_source::source_compiler_env(
        &request.intent.rust_version,
        request.git,
    )?);
    args.push(
        crate::release::crate_release_source::source_cargo_path(
            &request.intent.rust_version,
            home,
        )?
        .into_os_string(),
    );
    let catalog =
        crate::release::crate_release_source::source_tool_catalog(&request.intent.rust_version)
            .map_err(|_| invalid())?;
    args.extend([
        OsString::from("package"),
        "--list".into(),
        "--locked".into(),
        "--offline".into(),
        "--manifest-path".into(),
        request
            .source
            .join(&request.intent.manifest_path)
            .into_os_string(),
        "--package".into(),
        request.package.name.clone().into(),
        "--target-dir".into(),
        request.target.as_os_str().to_owned(),
    ]);
    let features = request
        .intent
        .publish_features
        .get(&request.package.name)
        .ok_or_else(invalid)?;
    if !features.is_empty() {
        args.extend(["--features".into(), features.join(",").into()]);
    }
    if prime_index {
        verify_capacity_rust(&request.intent.rust_version, &cargo_home, deadline)?;
        let online = args
            .iter()
            .filter(|arg| *arg != "--offline")
            .cloned()
            .collect();
        run_package_list_command(request, &catalog, online, deadline)?;
        verify_capacity_rust(&request.intent.rust_version, &cargo_home, deadline)?;
    }
    verify_capacity_rust(&request.intent.rust_version, &cargo_home, deadline)?;
    let output = run_package_list_command(request, &catalog, args, deadline)?;
    verify_capacity_rust(&request.intent.rust_version, &cargo_home, deadline)?;
    let mut paths = std::collections::BTreeSet::new();
    for path in output.split_terminator('\n') {
        if path.is_empty() || path.chars().any(char::is_control) || !paths.insert(path.to_owned()) {
            return Err(invalid());
        }
    }
    if paths.is_empty() || !output.ends_with('\n') {
        return Err(invalid());
    }
    Ok(paths.into_iter().collect())
}

pub(crate) fn verify_capacity_rust(
    rust: &str,
    cargo_home: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<(), OrchestratorError> {
    use std::ffi::OsString;
    let invalid = || internal("crate_release_toolchain_invalid");
    let runner = tempfile::tempdir().map_err(|_| invalid())?;
    let rustup = std::env::var_os("RUSTUP_HOME").ok_or_else(invalid)?;
    let cargo = crate::release::crate_release_source::source_cargo_path(rust, runner.path())?;
    let manifest = cargo
        .parent()
        .and_then(Path::parent)
        .ok_or_else(invalid)?
        .join("lib/rustlib/multirust-channel-manifest.toml");
    let cache = runner.path().join("velnor/rust-verified");
    std::fs::create_dir_all(&cache).map_err(|_| invalid())?;
    if manifest.is_file() {
        std::fs::copy(manifest, cache.join(format!("channel-rust-{rust}.toml")))
            .map_err(|_| invalid())?;
    }
    let catalog =
        crate::release::crate_release_source::source_tool_catalog(rust).map_err(|_| invalid())?;
    let verifier = if cfg!(all(target_os = "linux", target_arch = "x86_64"))
        && velnor_actions_workflow_renderer::verifier_builders::RUST_TREE_DIGESTS
            .iter()
            .any(|pin| pin.version() == rust && pin.target() == "x86_64-unknown-linux-gnu")
    {
        crate::workflow::verify_rust_owned_argv(
            &catalog,
            crate::workflow::ToolchainPhase::Installed,
        )
    } else {
        crate::workflow::verify_rust_argv(&catalog)
    };
    let archive_cache = cargo_home
        .parent()
        .ok_or_else(invalid)?
        .join("rust-archives");
    std::fs::create_dir_all(&archive_cache).map_err(|_| invalid())?;
    let mut rustup_env = OsString::from("RUSTUP_HOME=");
    rustup_env.push(rustup);
    let mut args = vec![
        OsString::from("-i"),
        format!("HOME={}", runner.path().display()).into(),
        format!("RUNNER_TEMP={}", runner.path().display()).into(),
        rustup_env,
        format!("CARGO_HOME={}", cargo_home.display()).into(),
        format!("VELNOR_RUST_ARCHIVE_CACHE={}", archive_cache.display()).into(),
        OsString::from("/bin/sh"),
    ];
    args.extend(verifier.into_iter().skip(1).map(OsString::from));
    let command = velnor_actions_mise::IsolatedCommand::repo_task("/usr/bin/env", args, &[])
        .map_err(|_| invalid())?;
    #[cfg(unix)]
    let output = command
        .run_quiescent_until(1024 * 1024, deadline)
        .map_err(|_| {
            #[cfg(test)]
            eprintln!("capacity verifier failure category: execution");
            invalid()
        })?
        .0;
    #[cfg(not(unix))]
    let output = command
        .run_until(1024 * 1024, deadline)
        .map_err(|_| invalid())?;
    if !output.success {
        #[cfg(test)]
        report_verifier_failure(&output.stderr);
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
fn report_verifier_failure(stderr: &[u8]) {
    let text = String::from_utf8_lossy(stderr);
    let category = [
        "rust_component_tarball_digest_mismatch",
        "rust_channel_manifest_digest_mismatch",
        "rust_component_not_in_manifest",
        "rust_toolchain_bytes_mismatch",
        "rust_component_archive_invalid",
        "curl: (",
    ]
    .into_iter()
    .find(|candidate| text.contains(candidate))
    .unwrap_or("other");
    eprintln!("capacity verifier failure category: {category}");
}

fn run_package_list_command(
    request: &crate::CratePackageSource<'_>,
    catalog: &velnor_actions_mise::ToolCatalog,
    args: Vec<std::ffi::OsString>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<String, OrchestratorError> {
    use velnor_actions_mise::{PinnedTool, PinnedToolExec};
    let invalid = || internal("crate_release_package_list_invalid");
    let command = PinnedToolExec::new(vec![PinnedTool::Rust], std::ffi::OsStr::new("env"), args)
        .map_err(|_| invalid())?
        .command(catalog)
        .map_err(|_| invalid())?
        .with_cwd(request.source.to_path_buf());
    #[cfg(unix)]
    let result = command
        .run_quiescent_until(32 * 1024 * 1024, deadline)
        .map_err(|_| invalid())?
        .0;
    #[cfg(not(unix))]
    let result = command
        .run_until(32 * 1024 * 1024, deadline)
        .map_err(|_| invalid())?;
    if !result.success {
        return Err(invalid());
    }
    result.stdout_text("cargo").map_err(|_| invalid())
}

#[cfg(all(test, unix))]
pub(super) fn seed_package_fixture(home: &Path) -> Result<bool, OrchestratorError> {
    let Some(snapshot) = std::env::var_os("VELNOR_NATIVE_REGISTRY_SNAPSHOT") else {
        return Ok(false);
    };
    crate::release::crate_release_semver::test_registry::copy_snapshot(Path::new(&snapshot), home)
        .map_err(|_| internal("crate_release_package_temp"))?;
    Ok(true)
}

pub(super) fn require_reviewer_owner<R: CrateRegistryRead>(
    registry: &R,
    owners: &[CrateRegistryOwner],
    reviewer_id: u64,
) -> Result<(), OrchestratorError> {
    for owner in owners {
        if owner.kind != "user" {
            continue;
        }
        if crate::crate_release_registry::linked_github_reviewer(registry, owner, reviewer_id)? {
            return Ok(());
        }
    }
    Err(internal("crate_release_reviewer_not_owner"))
}

pub(super) fn wait_for_tag<G: GitHubAdmissionRead>(
    github: &G,
    repository: &str,
    tag: &str,
    expected: &str,
) -> Result<String, OrchestratorError> {
    for attempt in 0..60 {
        match read_github_tag_evidence(repository, tag, github)? {
            Some(sha) if sha == expected => return Ok(sha),
            Some(_) => return Err(internal("crate_release_tag_mismatch")),
            None if attempt < 59 => std::thread::sleep(std::time::Duration::from_secs(2)),
            None => return Err(internal("crate_release_tag_missing")),
        }
    }
    Err(internal("crate_release_tag_missing"))
}

pub(super) fn wait_for_release<G: GitHubAdmissionRead>(
    github: &G,
    repository: &str,
    package: &CrateReleasePlannedPackage,
) -> Result<crate::crate_release_github::GitHubReleaseEvidence, OrchestratorError> {
    for attempt in 0..60 {
        match crate::crate_release_github::read_github_release_evidence(
            repository,
            &package.tag,
            &package.source.version,
            github,
        )? {
            Some(url) => return Ok(url),
            None if attempt < 59 => std::thread::sleep(std::time::Duration::from_secs(2)),
            None => return Err(internal("crate_release_github_release_missing")),
        }
    }
    Err(internal("crate_release_github_release_missing"))
}
