// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{preview_consumer_intent, preview_fixture};

#[test]
fn workspace_validation_rejects_partial_bootstrap_maps() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    preview_fixture(root)?;
    fs::create_dir_all(root.join("beta/src"))?;
    fs::write(
        root.join("beta/Cargo.toml"),
        "[package]\nname='beta'\nversion='0.1.0'\nedition='2021'\n",
    )?;
    fs::write(root.join("beta/src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['beta']\nresolver='2'\n[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname='beta'\nversion='0.1.0'\n\n[[package]]\nname='demo'\nversion='0.1.0'\n",
    )?;
    commit_fixture(root, "workspace")?;
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    assert!(output.status.success());
    let sha = String::from_utf8(output.stdout)?.trim().to_owned();
    let config_path = root.join(".velnor/config.toml");
    let base = fs::read_to_string(&config_path)?.replace(
        "packages = ['demo']",
        "publishable_workspace = true\nauthentication = 'bootstrap-token'",
    );
    let config = |packages: &str| {
        format!(
            "{base}\n[stacks.rust.release.new_crates]\nsource_sha = '{sha}'\n[stacks.rust.release.new_crates.packages]\n{packages}\n"
        )
    };
    fs::write(&config_path, config("demo = '0.1.0'"))?;
    commit_fixture(root, "partial map")?;
    let error =
        crate::validate_crate_release_workspace(root, None).expect_err("partial map accepted");
    assert!(
        error
            .to_string()
            .contains("creation_package_selection_mismatch"),
        "{error}"
    );
    fs::write(&config_path, config("demo = '0.1.0'\nbeta = '0.1.0'"))?;
    commit_fixture(root, "complete map")?;
    crate::validate_crate_release_workspace(root, None)?;
    fs::write(&config_path, config("demo = '0.1.0'\nbeta = '0.2.0'"))?;
    commit_fixture(root, "wrong version")?;
    let error =
        crate::validate_crate_release_workspace(root, None).expect_err("wrong version accepted");
    assert!(
        error
            .to_string()
            .contains("creation_package_version_mismatch"),
        "{error}"
    );
    Ok(())
}

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
    let intent =
        crate::release::crate_release_runtime::parse_crate_release_intent(intent.as_bytes())?;
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
    let nested =
        crate::release::crate_release_runtime::parse_crate_release_intent(nested.as_bytes())?;
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

#[test]
fn large_workspace_intent_includes_all_member_paths() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("checkout");
    fs::create_dir(&root)?;
    preview_fixture(&root)?;
    for index in 0..127 {
        let dir = root.join(format!("members/p{index:03}"));
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = 'member-{index:03}'\nversion = '0.1.0'\nedition = '2021'\npublish = false\n"
            ),
        )?;
        fs::write(dir.join("src/lib.rs"), "")?;
    }
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = ['members/*']\n\n[package]\nname = 'demo'\nversion = '0.1.0'\nedition = '2021'\n",
    )?;
    fs::remove_file(root.join("Cargo.lock"))?;
    let status = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&root)
        .status()?;
    assert!(status.success());
    commit_fixture(&root, "large workspace")?;
    let text = preview_consumer_intent(&root)?;
    let intent = crate::parse_crate_release_intent(text.as_bytes())?;
    assert_eq!(intent.pr_write_paths.len(), 257);
    assert!(
        intent
            .pr_write_paths
            .contains(&"members/p126/CHANGELOG.md".to_owned())
    );
    let mut oversized = intent.clone();
    oversized.pr_write_paths = (0..3000)
        .map(|i| format!("{i:04}{}/Cargo.toml", "'\"".repeat(200)))
        .collect();
    oversized.pr_write_paths.push("Cargo.toml".into());
    assert!(oversized.validate().is_err());
    assert!(crate::parse_crate_release_intent(&vec![b' '; 1024 * 1024 + 1]).is_err());
    Ok(())
}
