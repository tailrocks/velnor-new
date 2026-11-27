// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use anyhow::{Result, ensure};
use serde_json::Value;

use crate::mise_world::{SHA_PLACEHOLDER, World, job_fixture, real_action};

const REAL_ERROR: &str =
    "::error title=setup-velnor::version 'x' must be MAJOR.MINOR.PATCH, digits only";
const STEP_NAME: &str = "- name: Install velnor-ci";

fn job(world: &World, sha: &str, env: &[(&str, &str)]) -> Result<PathBuf> {
    let job = world.with_text(
        &job_fixture("setup_velnor_real_action"),
        &[(SHA_PLACEHOLDER, sha)],
    )?;
    if env.is_empty() {
        Ok(job)
    } else {
        world.with_job_env(&job, env)
    }
}

#[test]
fn setup_velnor_renders_the_outer_contexts_of_its_step() -> Result<()> {
    let world = World::new()?;
    let name = "${{ github.repository }}-${{ github.workspace }}-${{ env.OUTER }}-${{ runner.temp }}-${{ inputs.version }}-[${{ steps.leak.outputs.v }}]";
    let action = real_action()?.replace(STEP_NAME, &format!("- name: {name}"));
    let sha = world.publish_action(&action)?;
    let outer = job(&world, &sha, &[("OUTER", "from-job")])?;
    let leading = world.with_leading_script(&outer, "echo v=leaked >> \"$GITHUB_OUTPUT\"")?;
    let mut message: Value = serde_json::from_slice(&std::fs::read(&leading)?)?;
    message["Steps"][0]["ContextName"] = "leak".into();
    std::fs::write(&leading, serde_json::to_vec(&message)?)?;
    let ran = world.run(&leading, &[])?;
    let expected = format!(
        "setup-velnor step: tailrocks/velnor-new-{}-from-job-{}-x-[]",
        world.workspace().display(),
        world.temp().display()
    );
    ensure!(
        ran.has_line(&expected) && ran.has_line(REAL_ERROR),
        "{expected}\n{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn setup_velnor_refuses_a_context_it_cannot_supply() -> Result<()> {
    for root in ["secrets", "needs", "matrix", "job", "strategy", "vars"] {
        let world = World::new()?;
        let action = real_action()?.replace(STEP_NAME, &format!("- name: ${{{{ {root}.value }}}}"));
        let sha = world.publish_action(&action)?;
        let ran = world.run(&job(&world, &sha, &[])?, &[])?;
        let expected =
            format!("unsupported: setup-velnor expression: unsupported expression context {root}");
        ensure!(
            ran.has_line(&expected) && ran.has_line("step 1 failed"),
            "{expected}\n{}",
            ran.stdout
        );
        ensure!(
            !ran.stdout.contains("setup-velnor step:"),
            "{root}\n{}",
            ran.stdout
        );
    }
    Ok(())
}

#[test]
fn setup_velnor_refuses_an_action_without_steps() -> Result<()> {
    let action = real_action()?;
    let header = action
        .split_once("  steps:")
        .map_or_default(|(header, _)| header.to_owned());
    ensure!(header.ends_with("  using: composite\n"), "{header}");
    for (label, text) in [
        ("missing", header.clone()),
        ("empty", format!("{header}  steps: []\n")),
    ] {
        let world = World::new()?;
        let sha = world.publish_action(&text)?;
        let ran = world.run(&job(&world, &sha, &[])?, &[])?;
        ensure!(
            ran.has_line("setup-velnor action.yml declares no steps")
                && ran.has_line("step 1 failed"),
            "{label}\n{}",
            ran.stdout
        );
        ensure!(
            !ran.stdout.contains("setup-velnor step:"),
            "{label}\n{}",
            ran.stdout
        );
    }
    Ok(())
}

#[test]
fn setup_velnor_renders_the_shell_of_its_step() -> Result<()> {
    let world = World::new()?;
    let action = real_action()?.replace("shell: bash", "shell: ${{ env.CHOSEN }}");
    let sha = world.publish_action(&action)?;
    let ran = world.run(&job(&world, &sha, &[("CHOSEN", "bash")])?, &[])?;
    ensure!(ran.has_line(REAL_ERROR), "{}", ran.stdout);
    Ok(())
}
