// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::release::crate_release_source::CrateReleaseSourcePackage;

fn nested_fixture()
-> Result<(tempfile::TempDir, String, CrateReleaseIntent), Box<dyn std::error::Error>> {
    let (temp, _, mut intent) =
        fixture("[package]\nname='outer'\nversion='0.1.0'\nedition='2021'\n")?;
    let root = temp.path();
    fs::create_dir_all(root.join("selected/src"))?;
    fs::write(
        root.join("selected/src/lib.rs"),
        "pub fn selected() -> u8 { 42 }\n",
    )?;
    fs::write(
        root.join("selected/Cargo.toml"),
        "[workspace]\n[package]\nname='demo'\nversion='0.1.1'\nedition='2021'\nlicense='MIT'\ndescription='fixture'\n",
    )?;
    fs::write(
        root.join("selected/Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.1\"\n",
    )?;
    for (directory, content) in [
        ("broken", "[package\n"),
        (
            "git-dep",
            "[package]\nname='unrelated'\nversion='0.1.0'\n[dependencies]\nremote={git='https://invalid.example/repo'}\n",
        ),
        (
            "patch",
            "[workspace]\n[patch.crates-io]\nremote={git='https://invalid.example/repo'}\n",
        ),
    ] {
        fs::create_dir(root.join(directory))?;
        fs::write(root.join(directory).join("Cargo.toml"), content)?;
    }
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-qm", "nested workspace"])?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    intent.manifest_path = "selected/Cargo.toml".into();
    intent.pr_write_paths = vec![
        "selected/CHANGELOG.md".into(),
        "selected/Cargo.lock".into(),
        "selected/Cargo.toml".into(),
    ];
    Ok((temp, sha, intent))
}

#[test]
fn nested_selection_ignores_unrelated_manifests_for_metadata_and_package()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, sha, intent) = nested_fixture()?;
    let plan = crate_release_workspace_plan_with_registry(
        temp.path(),
        &sha,
        &intent,
        &NoRegistry,
        &test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    assert_eq!(plan.plan.graph.order, vec!["demo"]);
    assert_eq!(
        plan.plan.selection.packages[0].manifest,
        "selected/Cargo.toml"
    );
    let target = tempfile::tempdir()?;
    let archive = crate::crate_release_controller::package_unprivileged(
        temp.path(),
        &intent,
        &CrateReleaseSourcePackage {
            name: "demo".into(),
            version: "0.1.1".into(),
            manifest: "selected/Cargo.toml".into(),
        },
        target.path(),
        &sha,
        false,
        &test_git(),
    )?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(archive)));
    let paths: Vec<_> = tar
        .entries()?
        .map(|entry| entry.and_then(|entry| Ok(entry.path()?.into_owned())))
        .collect::<Result<_, _>>()?;
    assert!(paths.contains(&std::path::PathBuf::from("demo-0.1.1/src/lib.rs")));
    assert!(paths.contains(&std::path::PathBuf::from("demo-0.1.1/Cargo.lock")));
    assert!(paths.iter().all(|path| path.starts_with("demo-0.1.1")));
    Ok(())
}

#[test]
fn nested_selection_rejects_malformed_reachable_path_dependency()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, _, intent) = nested_fixture()?;
    let root = temp.path();
    fs::create_dir(root.join("selected/dep"))?;
    fs::write(root.join("selected/dep/Cargo.toml"), "[package\n")?;
    let manifest = root.join("selected/Cargo.toml");
    let mut content = fs::read_to_string(&manifest)?;
    content.push_str("[dependencies]\ndep={path='dep',version='0.1.0'}\n");
    fs::write(manifest, content)?;
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-qm", "invalid reachable dependency"])?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    let result = crate_release_workspace_plan_with_registry(
        root,
        &sha,
        &intent,
        &NoRegistry,
        &test_git(),
        &crate::UnprivilegedSourceExecution,
    );
    assert!(
        matches!(result, Err(OrchestratorError::Internal { problem }) if problem == "crate_release_metadata_failed")
    );
    Ok(())
}
