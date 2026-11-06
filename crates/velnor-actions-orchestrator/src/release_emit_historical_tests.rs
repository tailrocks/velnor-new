// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super as release;
use super::{preview_fixture, qualification::qualification_metadata};
use std::{fs, path::Path, process::Command};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn git(root: &Path, args: &[&str]) -> TestResult<String> {
    let output = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn commit(root: &Path, message: &str) -> TestResult<String> {
    git(root, &["add", "-A"])?;
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            message,
        ],
    )?;
    git(root, &["rev-parse", "HEAD"])
}

fn write_workspace(root: &Path, directory: &str, version: &str) -> TestResult<()> {
    fs::create_dir_all(root.join(directory).join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nmembers = ['{directory}']\nresolver = '2'\n"),
    )?;
    fs::write(
        root.join("Cargo.lock"),
        format!("version = 4\n\n[[package]]\nname = 'demo'\nversion = '{version}'\n"),
    )?;
    fs::write(
        root.join(directory).join("Cargo.toml"),
        format!("[package]\nname = 'demo'\nversion = '{version}'\nedition = '2021'\n"),
    )?;
    fs::write(
        root.join(directory).join("src/lib.rs"),
        "pub fn demo() {}\n",
    )?;
    Ok(())
}

fn setup_history(root: &Path) -> TestResult<(String, String, Vec<u8>)> {
    preview_fixture(root)?;
    fs::remove_dir_all(root.join("src"))?;
    write_workspace(root, "common/demo", "0.1.0")?;
    fs::create_dir_all(root.join("old/helper/src"))?;
    fs::write(
        root.join("old/helper/Cargo.toml"),
        "[package]\nname='helper'\nversion='0.1.0'\nedition='2021'\npublish=false\n",
    )?;
    fs::write(root.join("old/helper/src/lib.rs"), "pub fn helper() {}\n")?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['common/demo','old/helper']\nresolver='2'\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname='demo'\nversion='0.1.0'\n\n[[package]]\nname='helper'\nversion='0.1.0'\n",
    )?;
    let source_a = commit(root, "historical source")?;
    fs::remove_dir_all(root.join("old"))?;
    fs::remove_file(root.join("Cargo.toml"))?;
    fs::remove_file(root.join("Cargo.lock"))?;
    fs::write(
        root.join("common/Cargo.toml"),
        "[workspace]\nmembers=['demo']\nresolver='2'\n",
    )?;
    fs::write(
        root.join("common/Cargo.lock"),
        "version = 4\n\n[[package]]\nname='demo'\nversion='0.2.0'\n",
    )?;
    fs::write(
        root.join("common/demo/Cargo.toml"),
        "[package]\nname='demo'\nversion='0.2.0'\nedition='2021'\n",
    )?;
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?.replace(
        "packages = ['demo']",
        "packages = ['demo']\nmanifest_path='common/demo/Cargo.toml'",
    );
    fs::write(
        &config_path,
        format!(
            "{config}\n[stacks.rust.release.recovery]\npackage='demo'\nversion='0.1.0'\nsource_sha='{source_a}'\n"
        ),
    )?;
    let authority_b = commit(root, "current authority")?;
    assert_ne!(source_a, authority_b);
    let config_before = fs::read(&config_path)?;
    Ok((source_a, authority_b, config_before))
}

fn check_qualification(
    root: &Path,
    source_a: &str,
    prep: &release::GenerationPreparation,
    release_config: &release::RustReleaseConfig,
    derived: &release::ReleaseIntentInputs,
) -> TestResult<()> {
    let manifest = fs::read(root.join(".velnor/release-manifest.json"))?;
    let mut metadata_config = qualification_metadata(
        derived
            .catalog
            .version(velnor_actions_mise::PinnedTool::Rust),
    );
    metadata_config.manifest_path = release_config.manifest_path.clone();
    let snapshot = derived
        .source_snapshot
        .as_ref()
        .ok_or("source snapshot missing")?;
    let qualified = release::derive_intent_from_snapshot(
        snapshot,
        &prep.config,
        &derived.metadata,
        &manifest,
        &metadata_config,
        "example/demo",
        "main",
    )?;
    assert_eq!(
        toml::to_string(&derived.intent)?,
        toml::to_string(&qualified)?
    );
    let checkout_temp = tempfile::tempdir()?;
    let checkout = checkout_temp.path().join("source");
    git(
        root,
        &[
            "clone",
            "--no-hardlinks",
            root.to_str().ok_or("invalid root")?,
            checkout.to_str().ok_or("invalid checkout")?,
        ],
    )?;
    git(&checkout, &["checkout", "--detach", source_a])?;
    let source_git = crate::crate_release_git::ReleaseGit::system()?;
    let qualified_snapshot = crate::crate_release_qualification_snapshot::snapshot_approved_source(
        &checkout,
        source_a,
        &source_git,
    )?;
    let qualified_metadata = release::selection::release_metadata_json_at(
        prep,
        qualified_snapshot.root(),
        &derived.catalog,
        &release_config.manifest_path,
    )?;
    let verified = release::derive_intent_from_snapshot(
        &qualified_snapshot,
        &prep.config,
        &qualified_metadata,
        &manifest,
        &metadata_config,
        "example/demo",
        "main",
    )?;
    assert_eq!(
        toml::to_string(&derived.intent)?,
        toml::to_string(&verified)?
    );
    Ok(())
}

fn check_authority(
    intent: &release::CrateReleaseIntent,
    authority_b: &str,
    source_a: &str,
) -> TestResult<()> {
    let intent_file = tempfile::NamedTempFile::new()?;
    fs::write(intent_file.path(), toml::to_string(intent)?)?;
    let child = Command::new(std::env::current_exe()?)
        .env_clear()
        .env("VELNOR_TEST_HISTORICAL_INTENT", intent_file.path())
        .env("VELNOR_TEST_AUTHORITY", authority_b)
        .env("VELNOR_TEST_SOURCE", source_a)
        .env("GITHUB_REPOSITORY", "example/demo")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_WORKFLOW_REF", "example/demo/.github/workflows/release.yml@refs/heads/main")
        .env("GITHUB_WORKFLOW_SHA", authority_b)
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .args(["--exact", "release_emit::tests::historical::recovery_generation_and_qualification_use_historical_source_not_current_checkout", "--nocapture"])
        .output()?;
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    Ok(())
}

fn check_missing_and_creation(
    prep: &release::GenerationPreparation,
    release_config: &release::RustReleaseConfig,
    source_a: &str,
) -> TestResult<()> {
    let mut missing_object = release_config.clone();
    missing_object
        .recovery
        .as_mut()
        .ok_or("recovery missing")?
        .source_sha = "0".repeat(40);
    assert!(release::derive_release_inputs(prep, &missing_object).is_err());
    let mut missing_path = release_config.clone();
    missing_path.manifest_path = "missing/Cargo.toml".to_owned();
    assert!(release::derive_release_inputs(prep, &missing_path).is_err());
    let mut creation = release_config.clone();
    creation.recovery = None;
    creation.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: source_a.to_owned(),
        packages: [("demo".to_owned(), "0.1.0".to_owned())].into(),
    });
    let creation_inputs = release::derive_release_inputs(prep, &creation)?;
    assert_eq!(creation_inputs.source_sha, source_a);
    assert_eq!(creation_inputs.selection.packages[0].version, "0.1.0");
    Ok(())
}

#[test]
fn recovery_generation_and_qualification_use_historical_source_not_current_checkout()
-> TestResult<()> {
    if let Some(path) = std::env::var_os("VELNOR_TEST_HISTORICAL_INTENT") {
        let intent = toml::from_str(&fs::read_to_string(path)?)?;
        let authority = std::env::var("VELNOR_TEST_AUTHORITY")?;
        let source = std::env::var("VELNOR_TEST_SOURCE")?;
        let request =
            crate::crate_release_runtime::test_request_from_environment(&intent, &authority)?;
        assert_eq!(request.workflow_sha, authority);
        assert_eq!(request.source_sha, source);
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (source_a, authority_b, config_before) = setup_history(root)?;
    let config_path = root.join(".velnor/config.toml");
    let prep = crate::prepare::prepare_for_candidate(root)?;
    let release_config = release::enabled_release(&prep)?.ok_or("release missing")?;
    let derived = release::derive_release_inputs(&prep, release_config)?;
    assert_eq!(derived.source_sha, source_a);
    assert_eq!(derived.selection.packages[0].version, "0.1.0");
    assert!(
        derived
            .intent
            .pr_write_paths
            .contains(&"old/helper/Cargo.toml".to_owned())
    );
    assert!(
        !derived
            .intent
            .pr_write_paths
            .contains(&"common/Cargo.toml".to_owned())
    );
    let source_lock = check_historical_workspace_lock(root, &prep, release_config, &derived)?;
    check_qualification(root, &source_a, &prep, release_config, &derived)?;
    check_authority(&derived.intent, &authority_b, &source_a)?;
    git(
        root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/EXAMPLE/DEMO.git",
        ],
    )?;
    let mixed_prep = crate::prepare::prepare_for_candidate(root)?;
    let mixed_config = release::enabled_release(&mixed_prep)?.ok_or("release missing")?;
    let mixed = release::derive_release_inputs(&mixed_prep, mixed_config)?;
    assert_eq!(mixed.intent.repository, "EXAMPLE/DEMO");
    check_authority(&mixed.intent, &authority_b, &source_a)?;
    git(
        root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/example/demo.git",
        ],
    )?;
    check_missing_and_creation(&prep, release_config, &source_a)?;
    let mise = crate::pins::resolve_mise_setup(&prep.config, release::RELEASE_HOSTED_RUNNER)?;
    let files = release::release_files_for_config(&prep, &mise, release_config)?;
    let workflow = files
        .iter()
        .find(|file| file.path == ".github/workflows/release.yml")
        .ok_or("workflow missing")?;
    assert!(workflow.bytes.contains(&source_a));
    assert!(workflow.bytes.contains(&source_lock.group));
    assert_eq!(git(root, &["rev-parse", "HEAD"])?, authority_b);
    assert_eq!(fs::read(config_path)?, config_before);
    assert_eq!(git(root, &["status", "--porcelain"])?, "");
    Ok(())
}

#[cfg(unix)]
#[test]
fn historical_object_snapshot_rejects_escape_and_ignores_path_git() -> TestResult<()> {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    if let Some(root) = std::env::var_os("VELNOR_TEST_OBJECT_REPO") {
        let sha = std::env::var("VELNOR_TEST_OBJECT_SHA")?;
        let git = crate::crate_release_git::ReleaseGit::system()?;
        crate::crate_release_qualification_snapshot::snapshot_git_objects(
            Path::new(&root),
            &sha,
            &git,
        )?;
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    fs::create_dir(&root)?;
    preview_fixture(&root)?;
    fs::write(root.join("safe.txt"), "historical fixture")?;
    let source = commit(&root, "safe source")?;
    let bin = temp.path().join("bin");
    fs::create_dir(&bin)?;
    let fake = bin.join("git");
    let marker = temp.path().join("fake-ran");
    fs::write(
        &fake,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )?;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700))?;
    let child = Command::new(std::env::current_exe()?)
        .env_clear()
        .env("PATH", &bin)
        .env("VELNOR_TEST_OBJECT_REPO", &root)
        .env("VELNOR_TEST_OBJECT_SHA", &source)
        .args(["--exact", "release_emit::tests::historical::historical_object_snapshot_rejects_escape_and_ignores_path_git", "--nocapture"])
        .output()?;
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(!marker.exists());
    symlink("../../outside", root.join("escape"))?;
    let hostile = commit(&root, "escaping link")?;
    let git = crate::crate_release_git::ReleaseGit::system()?;
    assert!(
        crate::crate_release_qualification_snapshot::snapshot_git_objects(&root, &hostile, &git)
            .is_err()
    );
    assert!(!temp.path().join("outside").exists());
    Ok(())
}

fn check_historical_workspace_lock(
    root: &Path,
    prep: &release::GenerationPreparation,
    release_config: &release::RustReleaseConfig,
    derived: &release::ReleaseIntentInputs,
) -> TestResult<velnor_actions_workflow_renderer::release_spec::lock::ReleaseConcurrency> {
    let snapshot = derived
        .source_snapshot
        .as_ref()
        .ok_or("source snapshot missing")?;
    let source_lock = crate::release_identity::workspace_lock(
        &derived.metadata,
        snapshot.root(),
        &release_config.manifest_path,
        &derived.known,
        &derived.registry,
        &derived.repository,
    )?;
    let live_metadata = release::release_metadata_json(prep, &release_config.manifest_path)?;
    let live_known = release::selection::tracked_manifests(root)?;
    let live_lock = crate::release_identity::workspace_lock(
        &live_metadata,
        root,
        &release_config.manifest_path,
        &live_known,
        &derived.registry,
        &derived.repository,
    )?;
    assert_ne!(source_lock, live_lock);
    Ok(source_lock)
}
