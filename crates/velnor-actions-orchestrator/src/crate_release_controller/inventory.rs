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
    crate::crate_release_source::verify_toolchain_source_separation(source)?;
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
    let catalog = crate::crate_release_source::source_tool_catalog(&intent.rust_version)
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
    isolated_args.extend(crate::crate_release_source::source_compiler_env(
        &intent.rust_version,
        git,
    )?);
    isolated_args.push(
        crate::crate_release_source::source_cargo_path(&intent.rust_version, sterile.path())?
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

#[cfg(all(test, unix))]
pub(super) fn seed_package_fixture(home: &Path) -> Result<bool, OrchestratorError> {
    let Some(snapshot) = std::env::var_os("VELNOR_NATIVE_REGISTRY_SNAPSHOT") else {
        return Ok(false);
    };
    crate::crate_release_semver::test_registry::copy_snapshot(Path::new(&snapshot), home)
        .map_err(|_| internal("crate_release_package_temp"))?;
    Ok(true)
}

pub(super) fn require_reviewer_owner<G: GitHubAdmissionRead>(
    github: &G,
    owners: &[CrateRegistryOwner],
    reviewer_id: u64,
) -> Result<(), OrchestratorError> {
    for owner in owners {
        if owner.kind != "user"
            || !velnor_actions_contract::config::is_valid_github_owner(&owner.login)
        {
            continue;
        }
        let identity = github.get(&format!("users/{}", owner.login))?;
        if identity["id"].as_u64() == Some(reviewer_id)
            && identity["type"].as_str() == Some("User")
            && identity["login"]
                .as_str()
                .is_some_and(|login| login.eq_ignore_ascii_case(&owner.login))
        {
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
