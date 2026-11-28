// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};

use crate::mise_double::{each_row, git_bytes};
use crate::mise_repo::{REPO_MISE_CONFIG, REPO_MISE_LOCK, repo_config_without_wrappers};
use crate::mise_world::{CONFIG, World, assert_failed, git, job_fixture, pinned_mise_action};
use crate::native_mise_admission::assert_refused;

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
fn mise_action_uses_the_committed_bytes_whatever_the_work_tree_holds() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let workspace = world.workspace();
    let committed = git_bytes(&workspace, "mise.lock")?;
    let lock = workspace.join("mise.lock");
    let mut text = std::fs::read_to_string(&lock).context("read mise.lock")?;
    text.push_str("\n[[tools.ghost]]\nversion = \"1.0.0\"\n");
    std::fs::write(&lock, text).context("dirty mise.lock")?;
    let ran = world.run(&install_args_job(&world, "jactionlint ghost")?, &[])?;
    assert_failed(
        &ran,
        "mise install_args token 'ghost' is not in the committed mise.lock",
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert_eq!(world.calls()[0].files.get("mise.lock"), Some(&committed));
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
        assert_refused(
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
            &format!(
                "mise policy: {name} is missing from the trusted commit or is not a regular file"
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{name}");
    }
    Ok(())
}

const FOOTER: &str = "\n[settings]\nlockfile = true\n";

#[test]
fn mise_action_refuses_every_top_level_key_outside_the_allowed_set() -> Result<()> {
    let rows = [
        ("include", "include = [\"../../evil.toml\"]\n"),
        (
            "include",
            "include = [\"git::https://example.com/x//m.toml\"]\n",
        ),
        ("env_file", "env_file = \".env\"\n"),
        ("dotenv", "dotenv = \".env\"\n"),
        ("env_path", "env_path = [\"bin\"]\n"),
        ("hooks", "[hooks]\nenter = \"true\"\n"),
        ("plugins", "[plugins]\nfoo = \"https://example.com/foo\"\n"),
        ("task_config", "[task_config]\nincludes = [\"tasks\"]\n"),
        ("alias", "[alias.node]\nlts = \"26\"\n"),
        (
            "watch_files",
            "[[watch_files]]\npatterns = [\"x\"]\nrun = \"true\"\n",
        ),
        ("secrets", "[secrets]\nTOKEN = { command = \"true\" }\n"),
        ("wrappers", "[wrappers.cargo]\ncommand = \"mbx\"\n"),
    ];
    each_row(&rows, |(key, snippet)| {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace_with(&format!("{snippet}{FOOTER}"), true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_failed(
            &ran,
            &format!(
                "unsupported: mise policy: mise.toml key '{key}' is not supported by velnor-runner"
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{key}");
        Ok(())
    })
}

#[test]
fn mise_action_refuses_every_setting_outside_the_allowed_set() -> Result<()> {
    let rows = [
        ("env_file", "env_file = [\".env\"]\n"),
        ("rust.cargo_home", "[settings.rust]\ncargo_home = \"/c\"\n"),
        ("ceiling_paths", "ceiling_paths = []\n"),
        (
            "override_config_filenames",
            "override_config_filenames = [\"evil.toml\"]\n",
        ),
        ("trusted_config_paths", "trusted_config_paths = [\"/\"]\n"),
    ];
    each_row(&rows, |(key, snippet)| {
        let world = World::new()?;
        world.install_mise()?;
        let config =
            format!("[tools]\nnode = \"26.13.1\"\n\n[settings]\nlockfile = true\n{snippet}");
        world.init_workspace_with(&config, true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_failed(
            &ran,
            &format!(
                "unsupported: mise policy: [settings] key '{key}' is not supported by velnor-runner"
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{key}");
        Ok(())
    })
}

#[test]
fn mise_action_refuses_the_repository_mise_config_while_it_has_wrappers() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace_with_lock(REPO_MISE_CONFIG, REPO_MISE_LOCK)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(
        &ran,
        "unsupported: mise policy: mise.toml key 'wrappers' is not supported by velnor-runner",
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_accepts_the_repository_mise_config_without_wrappers() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace_with_lock(&repo_config_without_wrappers()?, REPO_MISE_LOCK)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert_eq!(world.mise_installs().len(), 1);
    Ok(())
}

#[test]
fn mise_action_refuses_a_config_or_lock_that_is_not_valid_utf8() -> Result<()> {
    let truncated: &[u8] = &[0xF0, 0x9F, 0x98];
    each_row(&["mise.toml", "mise.lock"], |name| {
        let world = World::new()?;
        world.install_mise()?;
        let comment = [b"# ", truncated, b"\n"].concat();
        let (config, lock) = if name == "mise.toml" {
            ([CONFIG.as_bytes(), &comment].concat(), Vec::new())
        } else {
            (CONFIG.as_bytes().to_vec(), comment)
        };
        world.write_workspace(&config, &lock, true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_failed(
            &ran,
            &format!("{name} in the trusted commit is not valid UTF-8"),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{name}");
        Ok(())
    })
}
