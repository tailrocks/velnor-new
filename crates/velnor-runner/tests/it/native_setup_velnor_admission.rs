// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::Result;

use crate::mise_world::{SHA_PLACEHOLDER, World, job_fixture, real_action};

fn refused_before_any_step(world: &World, job: &Path, expected: &str) -> Result<()> {
    let marker = world.temp().join("earlier-step-ran");
    let plant = format!("touch {}", marker.display());
    let ran = world.run(&world.with_leading_script(job, &plant)?, &[])?;
    anyhow::ensure!(
        ran.has_line(expected) && ran.has_line("job failed") && ran.code == Some(1),
        "{expected}\n{}",
        ran.stdout
    );
    anyhow::ensure!(!marker.exists(), "an earlier step ran\n{}", ran.stdout);
    anyhow::ensure!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn setup_velnor_refuses_an_unknown_input_before_any_step_runs() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let job = world.with_text(
        &job_fixture("setup_velnor_bad_input"),
        &[(SHA_PLACEHOLDER, &sha)],
    )?;
    refused_before_any_step(
        &world,
        &job,
        "unsupported: setup-velnor input 'bogus' is not supported by velnor-runner",
    )
}

#[test]
fn setup_velnor_refuses_a_missing_version_before_any_step_runs() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let job = world.with_text(
        &job_fixture("setup_velnor_no_version"),
        &[(SHA_PLACEHOLDER, &sha)],
    )?;
    refused_before_any_step(
        &world,
        &job,
        "unsupported: setup-velnor input 'version' is required",
    )
}
