// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::mise_world::{SHA_PLACEHOLDER, World, job_fixture, real_action};

fn with_expression(world: &World, job: &Path) -> Result<PathBuf> {
    let mut message: Value = serde_json::from_slice(&std::fs::read(job)?)?;
    message["Steps"][0]["Inputs"] = json!({"type": 3, "expr": "fromJSON('{}')"});
    let patched = world.root.join("expression-with.json");
    std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
    Ok(patched)
}

fn refused(world: &World, job: &Path, expected: &str) -> Result<()> {
    let marker = world.temp().join("earlier-step-ran");
    let plant = format!("touch {}", marker.display());
    let patched = with_expression(world, job)?;
    let ran = world.run(&world.with_leading_script(&patched, &plant)?, &[])?;
    ensure!(
        ran.has_line(expected) && ran.has_line("job failed") && ran.code == Some(1),
        "{expected}\n{}",
        ran.stdout
    );
    ensure!(!marker.exists(), "an earlier step ran\n{}", ran.stdout);
    ensure!(world.mise_calls().is_empty(), "mise ran\n{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_refuses_an_expression_valued_with_before_any_step_runs() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    refused(
        &world,
        &job_fixture("mise_action_ok"),
        "unsupported: jdx/mise-action cannot validate an expression-valued 'with:' in velnor-runner",
    )
}

#[test]
fn setup_velnor_refuses_an_expression_valued_with_before_any_step_runs() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let job = world.with_text(
        &job_fixture("setup_velnor_real_action"),
        &[(SHA_PLACEHOLDER, &sha)],
    )?;
    refused(
        &world,
        &job,
        "unsupported: setup-velnor cannot validate an expression-valued 'with:' in velnor-runner",
    )
}
