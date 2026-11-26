// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::mise_world::{World, assert_failed, job_fixture};

#[test]
fn mise_action_rejects_inputs_it_does_not_implement() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    for (name, value) in [
        ("install", "false"),
        ("env", "false"),
        ("working_directory", "sub"),
        ("mise_toml", "[tools]"),
    ] {
        let job = world.with_input(&job_fixture("mise_action_ok"), name, value)?;
        let ran = world.run(&job, &[])?;
        assert_failed(
            &ran,
            &format!(
                "unsupported: jdx/mise-action input '{name}' is not supported by velnor-runner"
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{name}");
    }
    Ok(())
}

#[test]
fn mise_action_treats_an_empty_unsupported_input_as_unset() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let mut job = job_fixture("mise_action_ok");
    for name in [
        "working_directory",
        "mise_toml",
        "log_level",
        "cache_save",
        "cache_save_post",
    ] {
        job = world.with_input(&job, name, "")?;
    }
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert_eq!(world.mise_installs().len(), 1);
    Ok(())
}

#[test]
fn mise_action_fails_an_empty_boolean_input_as_upstream_does() -> Result<()> {
    for name in [
        "install",
        "env",
        "reshim",
        "experimental",
        "add_shims_to_path",
        "Install",
    ] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let job = world.with_input(&job_fixture("mise_action_ok"), name, "")?;
        let ran = world.run(&job, &[])?;
        assert!(ran.has_line("job failed"), "{name}\n{}", ran.stdout);
        assert!(
            ran.stdout.contains(&format!(
                "Input does not meet YAML 1.2 \"Core Schema\" specification: {}",
                name.to_ascii_lowercase()
            )),
            "{name}\n{}",
            ran.stdout
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{name}");
    }
    Ok(())
}

#[test]
fn mise_action_matches_input_names_case_insensitively() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let without = world.without_input(&job_fixture("mise_action_ok"), "cache")?;
    let job = world.with_input(&without, "Cache", "false")?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_rejects_a_version_that_is_not_exact() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    for version in ["latest", "2026.11", "../../2026.11.10", "2026.13.1"] {
        let job = world.with_input(&job_fixture("mise_action_ok"), "version", version)?;
        let ran = world.run(&job, &[])?;
        assert_failed(
            &ran,
            &format!("mise version '{version}' is not an exact YYYY.M.D version"),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{version}");
    }
    Ok(())
}

#[test]
fn mise_action_rejects_cache_true() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_cache_true"), &[])?;
    assert_failed(
        &ran,
        "unsupported: jdx/mise-action input 'cache' must be false",
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_requires_cache_to_be_explicitly_false() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let omitted = world.without_input(&job_fixture("mise_action_ok"), "cache")?;
    let ran = world.run(&omitted, &[])?;
    assert_failed(
        &ran,
        "unsupported: jdx/mise-action input 'cache' must be false",
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_accepts_every_false_spelling_of_cache() -> Result<()> {
    for spelling in ["false", "False", "FALSE"] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        let job = world.with_input(&job_fixture("mise_action_ok"), "cache", spelling)?;
        let ran = world.run(&job, &[])?;
        assert!(ran.has_line("job succeeded"), "{spelling}\n{}", ran.stdout);
    }
    Ok(())
}

#[test]
fn mise_action_publishes_cache_hit_false_when_the_install_fails() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.with_final_script(
        &job_fixture("mise_action_ok"),
        "echo \"hit=${{ steps.mise.outputs.cache-hit }}\"",
    )?;
    let job = world.with_final_condition(&job, "always()")?;
    let ran = world.run(&job, &[("MISE_DOUBLE_INSTALL_EXIT", "1")])?;
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    assert!(ran.has_line("hit=false"), "{}", ran.stdout);
    Ok(())
}
