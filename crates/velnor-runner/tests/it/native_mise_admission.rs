// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};

use crate::exec_support::Ran;
use crate::mise_world::{
    SHA_PLACEHOLDER, VERSION, World, job_fixture, pinned_mise_action, real_action,
};

pub(crate) fn assert_refused(ran: &Ran, expected: &str) {
    assert!(ran.has_line(expected), "{expected}\n{}", ran.stdout);
    assert!(ran.has_line("job failed"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("step 1"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
}

fn marker(world: &World) -> PathBuf {
    world.temp().join("earlier-step-ran")
}

fn prepared() -> Result<World> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    Ok(world)
}

fn refused_before_any_step(world: &World, job: &Path, expected: &str) -> Result<()> {
    let plant = format!("touch {}", marker(world).display());
    let leading = world.with_leading_script(job, &plant)?;
    let ran = world.run(&leading, &[])?;
    ensure!(
        ran.stdout.contains(expected) && ran.has_line("job failed") && ran.code == Some(1),
        "{expected}\n{}",
        ran.stdout
    );
    ensure!(
        !marker(world).exists(),
        "an earlier step ran: {expected}\n{}",
        ran.stdout
    );
    ensure!(world.mise_calls().is_empty(), "mise ran: {expected}");
    Ok(())
}

const CACHE: &str = "unsupported: jdx/mise-action input 'cache' must be false";

#[test]
fn mise_action_is_refused_before_any_step_runs() -> Result<()> {
    let pinned = pinned_mise_action()?;
    let other = "0123456789abcdef0123456789abcdef01234567";
    let ok = job_fixture("mise_action_ok");
    let world = prepared()?;
    refused_before_any_step(&world, &job_fixture("mise_action_cache_true"), CACHE)?;
    refused_before_any_step(&world, &world.without_input(&ok, "cache")?, CACHE)?;
    let moved = world.with_text(&ok, &[(&pinned, other)])?;
    let revision = format!(
        "unsupported: jdx/mise-action@{other} is not the jdx/mise-action revision {pinned} pinned in .velnor/version-policy.toml"
    );
    refused_before_any_step(&world, &moved, &revision)?;
    for (name, value, expected) in [
        (
            "mise_toml",
            "[tools]",
            "unsupported: jdx/mise-action input 'mise_toml' is not supported by velnor-runner"
                .to_owned(),
        ),
        (
            "version",
            "2026.11.14",
            format!(
                "unsupported: mise 2026.11.14 is not the mise version {VERSION} pinned in .velnor/version-policy.toml"
            ),
        ),
        (
            "version",
            "latest",
            "unsupported: mise version 'latest' is not an exact YYYY.M.D version".to_owned(),
        ),
        (
            "install",
            "",
            "unsupported: Input does not meet YAML 1.2 \"Core Schema\" specification: install"
                .to_owned(),
        ),
    ] {
        let job = world.with_input(&ok, name, value)?;
        refused_before_any_step(&world, &job, &expected)?;
    }
    Ok(())
}

#[test]
fn mise_action_leaves_a_computed_input_to_its_step() -> Result<()> {
    let world = prepared()?;
    let job = world.with_input(
        &job_fixture("mise_action_ok"),
        "version",
        &format!("${{{{ '{VERSION}' }}}}"),
    )?;
    let job = world.with_input(&job, "cache", "${{ 'false' }}")?;
    let ran = world.run(&job, &[])?;
    ensure!(ran.has_line("job succeeded"), "{}", ran.stdout);
    ensure!(world.mise_installs().len() == 1, "{:?}", world.mise_calls());
    let job = world.with_input(
        &job_fixture("mise_action_ok"),
        "version",
        "${{ '2026.11.14' }}",
    )?;
    let ran = world.run(&job, &[])?;
    ensure!(
        ran.has_line(&format!(
            "unsupported: mise 2026.11.14 is not the mise version {VERSION} pinned in .velnor/version-policy.toml"
        )) && ran.has_line("step 1 failed"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn setup_velnor_is_refused_before_any_step_runs() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let fixture = job_fixture("setup_velnor_real_action");
    let moved = world.with_text(&fixture, &[(SHA_PLACEHOLDER, "main")])?;
    refused_before_any_step(
        &world,
        &moved,
        "unsupported: setup-velnor reference 'main' is not a 40 digit commit sha",
    )?;
    let fork = world.with_text(
        &fixture,
        &[
            (SHA_PLACEHOLDER, &sha),
            ("\"s\": \"tailrocks/velnor-new\"", "\"s\": \"someone/fork\""),
        ],
    )?;
    refused_before_any_step(
        &world,
        &fork,
        "unsupported: setup-velnor is only supported in tailrocks/velnor-new, not someone/fork",
    )
}
