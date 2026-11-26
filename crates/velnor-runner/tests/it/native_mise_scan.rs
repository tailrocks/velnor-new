// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Context, Result};

use crate::exec_support::Ran;
use crate::mise_world::{World, assert_failed, job_fixture};

const WOULD_LOAD: &str = "unsupported: mise policy: mise would load [";

fn plant(file: &Path, text: &str) -> Result<()> {
    std::fs::create_dir_all(file.with_file_name("")).context("create the directory")?;
    std::fs::write(file, text).context("plant the file")
}

fn assert_rejected_before_mise(world: &World, ran: &Ran, file: &Path, label: &str) -> Result<()> {
    let loaded = std::fs::canonicalize(file).context("resolve the planted file")?;
    let line = ran
        .lines()
        .into_iter()
        .find(|line| line.starts_with(WOULD_LOAD))
        .with_context(|| format!("{label}\n{}", ran.stdout))?;
    assert!(
        line.contains(&loaded.display().to_string()),
        "{label}: {line}"
    );
    assert!(ran.has_line("step 1 failed"), "{label}\n{}", ran.stdout);
    assert_eq!(ran.code, Some(1), "{label}");
    assert_eq!(world.mise_calls(), Vec::<String>::new(), "{label}");
    Ok(())
}

#[test]
fn mise_action_never_runs_mise_when_a_planted_config_would_execute_templates() -> Result<()> {
    let cases = [
        (
            "vars",
            "[vars]\nV = \"{{ exec(command='touch MARKER') }}\"\n",
        ),
        (
            "tools",
            "[tools]\nnode = \"{{ exec(command='touch MARKER') }}\"\n",
        ),
        ("env", "[env]\nV = \"{{ exec(command='touch MARKER') }}\"\n"),
    ];
    for (label, config) in cases {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        std::fs::create_dir_all(world.temp()).context("create the temp directory")?;
        let marker = world.temp().join("exec-ran");
        let planted = world.workspace().join("mise.local.toml");
        plant(
            &planted,
            &config.replace("MARKER", &marker.display().to_string()),
        )?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_rejected_before_mise(&world, &ran, &planted, label)?;
        assert!(!marker.exists(), "{label}: mise ran the planted exec()");
    }
    Ok(())
}

#[test]
fn mise_action_rejects_every_config_location_whatever_the_environment_name() -> Result<()> {
    let planted = [
        "mise.ci.local.toml",
        "mise.production.toml",
        ".mise.ci.toml",
        ".mise.local.toml",
        ".config/mise.ci.toml",
        ".config/mise.local.toml",
        ".config/mise/config.local.toml",
        ".config/mise/config.ci.toml",
        ".config/mise/mise.toml",
        ".config/mise/conf.d/50.ci.toml",
        ".config/mise/conf.d/git/mise.toml",
        ".config/mise/conf.d/git/mise.ci.toml",
        ".mise/conf.d/extra.toml",
        ".mise/config.local.toml",
        "mise/config.local.toml",
        "mise/conf.d/git/mise.local.toml",
        ".miserc.toml",
        ".miserc.local.toml",
        ".config/miserc.toml",
    ];
    for name in planted {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let file = world.workspace().join(name);
        plant(&file, "[env]\nPLANTED = \"1\"\n")?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_rejected_before_mise(&world, &ran, &file, name)?;
    }
    Ok(())
}

#[test]
fn mise_action_rejects_global_and_system_config() -> Result<()> {
    type Locate = fn(&World) -> std::path::PathBuf;
    let cases: [(&str, Locate); 6] = [
        ("global config.toml", |world| {
            world.home().join(".config/mise/config.toml")
        }),
        ("global config.local.toml", |world| {
            world.home().join(".config/mise/config.local.toml")
        }),
        ("global override", |world| {
            world.home().join(".config/mise/config.override.toml")
        }),
        ("global conf.d", |world| {
            world.home().join(".config/mise/conf.d/x.toml")
        }),
        ("home .tool-versions", |world| {
            world.home().join(".tool-versions")
        }),
        ("system config", |world| {
            world.system_config().join("config.toml")
        }),
    ];
    for (label, locate) in cases {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let file = locate(&world);
        plant(&file, "[env]\nPLANTED = \"1\"\n")?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert_rejected_before_mise(&world, &ran, &file, label)?;
    }
    Ok(())
}

#[test]
fn mise_action_rejects_config_files_named_by_the_environment() -> Result<()> {
    for variable in [
        "MISE_CONFIG_FILE",
        "MISE_GLOBAL_CONFIG_FILE",
        "MISE_SYSTEM_CONFIG_FILE",
    ] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let file = world.temp().join("elsewhere.toml");
        plant(&file, "[env]\nPLANTED = \"1\"\n")?;
        let text = file.display().to_string();
        let ran = world.run(&job_fixture("mise_action_ok"), &[(variable, &text)])?;
        assert_rejected_before_mise(&world, &ran, &file, variable)?;
    }
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let directory = world.temp().join("config-dir");
    let file = directory.join("config.toml");
    plant(&file, "[env]\nPLANTED = \"1\"\n")?;
    let text = directory.display().to_string();
    let ran = world.run(
        &job_fixture("mise_action_ok"),
        &[("MISE_CONFIG_DIR", &text)],
    )?;
    assert_rejected_before_mise(&world, &ran, &file, "MISE_CONFIG_DIR")
}

#[test]
fn mise_action_rejects_variables_that_redefine_the_config_filenames() -> Result<()> {
    for (variable, value) in [
        ("MISE_OVERRIDE_CONFIG_FILENAMES", "evil.toml"),
        ("MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES", "evil-versions"),
        ("MISE_IDIOMATIC_VERSION_FILE_ENABLE_TOOLS", "node"),
    ] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[(variable, value)])?;
        assert_failed(
            &ran,
            &format!(
                "unsupported: mise policy: {variable} changes which files mise loads, which velnor-runner does not support"
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{variable}");
    }
    Ok(())
}

#[test]
fn mise_action_rejects_idiomatic_version_files_enabled_by_the_workspace_config() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    let config = "[settings]\nlockfile = true\nidiomatic_version_file_enable_tools = [\"node\"]\n";
    world.init_workspace_with(config, true)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(
        &ran,
        "unsupported: mise policy: mise.toml sets idiomatic_version_file_enable_tools, so mise would load idiomatic version files, which velnor-runner does not support",
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_stops_the_config_search_at_the_ceiling() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let parent = world.workspace().with_file_name("");
    plant(&parent.join("mise.toml"), "[env]\nOUTSIDE = \"1\"\n")?;
    let ceiling = parent.display().to_string();
    let ceiling = ceiling.trim_end_matches('/');
    let ran = world.run(
        &job_fixture("mise_action_ok"),
        &[("MISE_CEILING_PATHS", ceiling)],
    )?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    Ok(())
}
