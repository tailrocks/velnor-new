// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use super::bun_private_credentials_support::{assert_runner, private_bun_repo, set_execution};
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

#[test]
fn private_producers_reject_every_profile_except_designated_hosted() -> Result<(), Box<dyn Error>> {
    for source in ["local", "hosted"] {
        let repo = private_bun_repo()?;
        set_execution(repo.path(), "local", &[("bun-fixture-ui--assets", "alias")])?;
        let path = repo.path().join(".velnor/config.toml");
        let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path)?)?;
        let profiles = config
            .get_mut("execution")
            .and_then(|value| value.get_mut("profiles"))
            .and_then(toml::Value::as_table_mut)
            .ok_or("execution profiles are missing")?;
        let alias = profiles
            .get(source)
            .ok_or("source profile is missing")?
            .clone();
        profiles.insert("alias".to_owned(), alias);
        fs::write(path, toml::to_string(&config)?)?;
        let result =
            prepare(repo.path()).and_then(|prepared| render_staged_tree_with(&prepared, None));
        let error = result
            .err()
            .ok_or("an undesignated producer profile was accepted")?;
        let error = error.to_string();
        assert!(error.contains("execution.overrides.bun-fixture-ui--assets.profile"));
        assert!(error.contains("bun_private_assets_hosted_only"), "{error}");
    }
    Ok(())
}

#[test]
fn private_producer_accepts_designated_hosted_with_local_consumer() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    set_execution(
        repo.path(),
        "local",
        &[("bun-fixture-ui--assets", "hosted")],
    )?;
    let rendered = render_staged_tree_with(&prepare(repo.path())?, None)?;
    let workflow = rendered.get(WORKFLOW_PATH).ok_or("workflow is missing")?;
    assert_runner(
        workflow,
        "bun-fixture-ui",
        "[velnor, ubuntu-26.04-scale-set]",
    )?;
    assert_runner(workflow, "bun-fixture-ui--assets", "ubuntu-26.04")?;
    Ok(())
}

#[test]
fn maximum_scoped_member_keeps_its_private_handoff() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let name = format!("@{}/{}", "a".repeat(64), "b".repeat(64));
    for relative in [
        ".velnor/config.toml",
        "package.json",
        "bun.lock",
        "apps/web/package.json",
        "packages/ui/package.json",
    ] {
        let path = repo.path().join(relative);
        let content = fs::read_to_string(&path)?.replace("@fixture/ui", &name);
        fs::write(path, content)?;
    }
    let rendered = render_staged_tree_with(&prepare(repo.path())?, None)?;
    let workflow = rendered.get(WORKFLOW_PATH).ok_or("workflow is missing")?;
    let id = format!("bun-{}-{}", "a".repeat(64), "b".repeat(64));
    assert_eq!(id.len(), 133);
    super::bun_private_credentials_support::assert_member_handoff(workflow, &id)?;
    assert_runner(workflow, &format!("{id}--assets"), "ubuntu-26.04")?;
    Ok(())
}
