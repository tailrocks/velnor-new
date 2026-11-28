// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Result, ensure};

use crate::mise_double::prepared;
use crate::mise_world::job_fixture;

const SCRIPT: &str = "echo \"later=$MISE_YES $MISE_TRUSTED_CONFIG_PATHS\"";

#[test]
fn mise_action_defaults_ignore_the_runner_process_environment() -> Result<()> {
    let world = prepared()?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), SCRIPT)?;
    let ran = world.run(
        &job,
        &[("MISE_YES", "0"), ("MISE_TRUSTED_CONFIG_PATHS", "/process")],
    )?;
    let expected = format!("later=1 {}", world.workspace().display());
    ensure!(ran.has_line(&expected), "{expected}\n{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_defaults_replace_an_empty_job_value() -> Result<()> {
    let world = prepared()?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), SCRIPT)?;
    let job = world.with_job_env(&job, &[("MISE_YES", ""), ("MISE_TRUSTED_CONFIG_PATHS", "")])?;
    let ran = world.run(&job, &[])?;
    let expected = format!("later=1 {}", world.workspace().display());
    ensure!(ran.has_line(&expected), "{expected}\n{}", ran.stdout);
    Ok(())
}
