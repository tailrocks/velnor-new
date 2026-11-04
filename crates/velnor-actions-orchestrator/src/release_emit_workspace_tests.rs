// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{preview_consumer_intent, preview_fixture};

#[test]
fn release_intent_includes_nested_auto_workspace_members() -> Result<(), Box<dyn std::error::Error>>
{
    use std::fs;
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    fs::create_dir(&root)?;
    preview_fixture(&root)?;
    fs::create_dir_all(root.join(".private/dependency/src"))?;
    fs::write(
        root.join(".private/dependency/Cargo.toml"),
        "[package]\nname = 'dependency'\nversion = '0.1.0'\nedition = '2021'\n",
    )?;
    fs::write(
        root.join(".private/dependency/src/lib.rs"),
        "pub fn dependency() {}\n",
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[package]\nname = 'demo'\nversion = '0.1.0'\nedition = '2021'\n\n[dependencies]\ndependency = { path = '.private/dependency' }\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = 'demo'\nversion = '0.1.0'\ndependencies = [\n 'dependency',\n]\n\n[[package]]\nname = 'dependency'\nversion = '0.1.0'\n",
    )?;
    commit_fixture(&root, "workspace")?;
    let intent = preview_consumer_intent(&root)?;
    let intent = crate::crate_release_runtime::parse_crate_release_intent(intent.as_bytes())?;
    assert_eq!(
        intent.pr_write_paths,
        [
            ".private/dependency/CHANGELOG.md",
            ".private/dependency/Cargo.toml",
            "CHANGELOG.md",
            "Cargo.lock",
            "Cargo.toml"
        ]
    );
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    fs::write(
        &config_path,
        config.replace(
            "[stacks.rust.release]",
            "[stacks.rust.release]\nmanifest_path = '.private/dependency/Cargo.toml'",
        ),
    )?;
    commit_fixture(&root, "nested manifest")?;
    let nested = preview_consumer_intent(&root)?;
    let nested = crate::crate_release_runtime::parse_crate_release_intent(nested.as_bytes())?;
    assert_eq!(nested.manifest_path, ".private/dependency/Cargo.toml");
    assert_eq!(nested.pr_write_paths, intent.pr_write_paths);
    Ok(())
}

fn commit_fixture(root: &std::path::Path, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    for args in [
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            message,
        ],
    ] {
        if !std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()?
            .success()
        {
            return Err("fixture git failed".into());
        }
    }
    Ok(())
}
