// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::workflow_configuration_fixture::git_fixture;
use velnor_actions_orchestrator::migrate_config;

use super::bun_routing::make_bun_repo;

pub(crate) struct BunPlanningRepo {
    pub(crate) repo: TempDir,
    pub(crate) base: String,
    pub(crate) head: String,
    pub(crate) merge: String,
}

#[derive(Clone, Copy)]
pub(crate) enum ChangedMember {
    Web,
    Ui,
    Docs,
    SuiteOnly,
}

pub(crate) fn make_bun_planning_repo(
    changed: ChangedMember,
) -> Result<BunPlanningRepo, Box<dyn Error>> {
    make_bun_planning_repo_with_scopes(changed, false)
}

pub(crate) fn make_bun_planning_repo_with_scopes(
    changed: ChangedMember,
    private: bool,
) -> Result<BunPlanningRepo, Box<dyn Error>> {
    let repo = make_bun_repo()?;
    let root = repo.path();
    migrate_config(root, 2, true)?;
    configure_planning_repo(root, private)?;
    add_web_workspace_dependency(root)?;
    if matches!(changed, ChangedMember::SuiteOnly) {
        let path = root.join(".velnor/config.toml");
        let config = fs::read_to_string(&path)?;
        let original = "scripts = [\"typecheck\", \"lint\", \"test\", \"test:browser\"]";
        if config.matches(original).count() != 1 {
            return Err("web script configuration changed".into());
        }
        fs::write(
            path,
            config.replace(original, "scripts = [\"test\", \"test:browser\"]"),
        )?;
    }
    git(root, &["branch", "-m", "main"])?;
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-m", "planning fixture base"])?;
    let base = git(root, &["rev-parse", "HEAD"])?;
    git(root, &["checkout", "-b", "planning-change"])?;
    change_member(root, changed)?;
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-m", "change Bun workspace member"])?;
    let head = git(root, &["rev-parse", "HEAD"])?;
    git(root, &["checkout", "main"])?;
    git(
        root,
        &["merge", "--no-ff", "planning-change", "-m", "PR merge"],
    )?;
    let merge = git(root, &["rev-parse", "HEAD"])?;
    Ok(BunPlanningRepo {
        repo,
        base,
        head,
        merge,
    })
}

fn configure_planning_repo(root: &Path, private: bool) -> Result<(), Box<dyn Error>> {
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    let marker = "default_profile = \"hosted\"";
    if config.matches(marker).count() != 1 || config.contains("mode = ") {
        return Err("migrated execution defaults changed".into());
    }
    let mut config = config.replace(marker, "default_profile = \"hosted\"\nmode = \"both\"");
    config.push_str(
        r#"

[private_sources]
owner = "fixture-owner"
app_id_variable = "FIXTURE_APP_ID"
app_private_key_secret = "FIXTURE_APP_PRIVATE_KEY"

[[private_sources.release_assets]]
id = "fixture-assets"
repository = "fixture-owner/fixture-assets"
tag = "v1.0.0"
asset = "fixture-assets.tar.gz"
extract_to = "fixture-assets"
"#,
    );
    if private {
        config = config.replace(
            "[private_sources]",
            "[private_sources]\nnpm_scopes = [\"@fixture\"]",
        );
    }
    fs::write(config_path, config)?;
    fs::write(
        root.join(".velnor/deps.lock"),
        r#"schema = 1

[[release_assets]]
id = "fixture-assets"
repository = "fixture-owner/fixture-assets"
tag = "v1.0.0"
commit = "0123456789abcdef0123456789abcdef01234567"
asset = "fixture-assets.tar.gz"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
extract_to = "fixture-assets"
"#,
    )?;
    Ok(())
}

fn add_web_workspace_dependency(root: &Path) -> Result<(), Box<dyn Error>> {
    let path = root.join("apps/web/package.json");
    let mut package: Value = serde_json::from_slice(&fs::read(&path)?)?;
    let dependencies = package
        .as_object_mut()
        .ok_or("web package manifest is not an object")?
        .entry("dependencies")
        .or_insert_with(|| json!({}));
    dependencies
        .as_object_mut()
        .ok_or("web package dependencies are not an object")?
        .insert("@fixture/ui".to_owned(), json!("workspace:*"));
    fs::write(path, format!("{}\n", serde_json::to_string(&package)?))?;
    Ok(())
}

fn change_member(root: &Path, changed: ChangedMember) -> Result<(), Box<dyn Error>> {
    if matches!(changed, ChangedMember::Docs) {
        fs::create_dir_all(root.join("docs"))?;
        fs::write(root.join("docs/guide.md"), "changed documentation\n")?;
        return Ok(());
    }
    let relative = match changed {
        ChangedMember::Web | ChangedMember::SuiteOnly => "apps/web/package.json",
        ChangedMember::Ui => "packages/ui/package.json",
        ChangedMember::Docs => unreachable!(),
    };
    let path = root.join(relative);
    let mut package: Value = serde_json::from_slice(&fs::read(&path)?)?;
    package
        .as_object_mut()
        .ok_or("workspace package manifest is not an object")?
        .insert(
            "description".to_owned(),
            json!("changed in planning fixture"),
        );
    fs::write(path, format!("{}\n", serde_json::to_string(&package)?))?;
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = git_fixture::command(root)?.args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
