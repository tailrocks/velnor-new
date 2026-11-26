// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};

use crate::mise_world::{CONFIG, World, assert_failed, git, job_fixture, pinned_mise_action};

const LISTED: &str = "jactionlint shellcheck node";

fn install_args_job(world: &World, words: &str) -> Result<std::path::PathBuf> {
    world.with_text(&job_fixture("mise_action_install_args"), &[(LISTED, words)])
}

#[test]
fn mise_action_rejects_install_args_token_not_in_mise_lock() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&install_args_job(&world, "jactionlint ghost")?, &[])?;
    assert_failed(
        &ran,
        "mise install_args token 'ghost' is not in the committed mise.lock",
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_rejects_install_args_token_that_is_not_a_bare_tool_key() -> Result<()> {
    for token in ["node@26.13.1", "--force", "../node"] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let ran = world.run(
            &install_args_job(&world, &format!("shellcheck {token}"))?,
            &[],
        )?;
        assert_failed(
            &ran,
            &format!("mise install_args token '{token}' is not a bare tool key"),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{token}");
    }
    Ok(())
}

#[test]
fn mise_action_rejects_dirty_mise_lock() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let lock = world.workspace().join("mise.lock");
    let mut text = std::fs::read_to_string(&lock).context("read mise.lock")?;
    text.push_str("\n[[tools.ghost]]\nversion = \"1.0.0\"\n");
    std::fs::write(&lock, text).context("dirty mise.lock")?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(&ran, "mise policy: mise.lock is dirty against index/HEAD");
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_requires_the_lockfile_setting_to_be_true() -> Result<()> {
    for config in [
        "[settings]\nlockfile = false\n\n[tools]\nnode = \"26.13.1\"\n",
        "[tools]\nnode = \"26.13.1\"\n",
    ] {
        assert_ne!(config, CONFIG);
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace_with(config, true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_failed(
            &ran,
            "mise policy: mise.toml does not set [settings] lockfile = true",
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new());
    }
    Ok(())
}

#[test]
fn mise_action_ignores_git_from_the_step_path() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.plant_fake_git(&job_fixture("mise_action_ok"))?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert!(!world.fake_git_marker().exists(), "the fake git ran");
    assert_eq!(world.mise_installs().len(), 1);
    Ok(())
}

#[test]
fn mise_action_accepts_only_the_pinned_revision() -> Result<()> {
    let pinned = pinned_mise_action()?;
    for other in ["0123456789abcdef0123456789abcdef01234567", "main", "v5.2.9"] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let job = world.with_text(&job_fixture("mise_action_ok"), &[(&pinned, other)])?;
        let ran = world.run(&job, &[])?;
        assert_failed(
            &ran,
            &format!(
                "unsupported: jdx/mise-action@{other} is not the jdx/mise-action revision {pinned} pinned in .velnor/version-policy.toml"
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{other}");
    }
    Ok(())
}

#[test]
fn mise_action_rejects_a_symlinked_mise_config_or_lock() -> Result<()> {
    for name in ["mise.toml", "mise.lock"] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let workspace = world.workspace();
        let target = format!("{name}.real");
        std::fs::rename(workspace.join(name), workspace.join(&target)).context("move the file")?;
        std::os::unix::fs::symlink(&target, workspace.join(name)).context("link the file")?;
        git(&workspace, &["add", "--all"])?;
        git(&workspace, &["commit", "--quiet", "--message", "link"])?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_failed(
            &ran,
            &format!("mise policy: {name} is missing from the workspace or is not a regular file"),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{name}");
    }
    Ok(())
}
