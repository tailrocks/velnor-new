// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use crate::exec_support::Ran;
use crate::mise_double::prepared;
use crate::mise_real::canonical;
use crate::mise_world::{World, assert_failed, job_fixture};

const ROOT: &str = "1 0 0:1 / / rw - ext4 /dev/a rw";

fn nested_line(world: &World) -> String {
    let store = canonical(&world.store).display().to_string();
    format!("41 1 0:50 / {store}/tools/data/installs rw - ext4 /dev/b rw")
}

fn expected(world: &World) -> String {
    let data = canonical(&world.store).join("tools/data");
    format!(
        "mise policy: MISE_DATA_DIR {} holds a writable mount at {}/installs; the tools store must be one read-only mount",
        data.display(),
        data.display()
    )
}

fn table(world: &World) -> Result<PathBuf> {
    let file = world.root.join("mountinfo");
    std::fs::write(&file, format!("{ROOT}\n")).context("write the mount table")?;
    Ok(file)
}

fn append_script(world: &World, table: &Path, name: &str) -> Result<PathBuf> {
    let script = world.root.join(name);
    let line = nested_line(world);
    std::fs::write(
        &script,
        format!("printf '%s\\n' '{line}' >> {}\n", table.display()),
    )
    .context("write the mount hook")?;
    Ok(script)
}

fn run_with_table(world: &World, job: &Path, table: &Path) -> Result<Ran> {
    let declared = table.display().to_string();
    world.run(job, &[("VELNOR_TEST_MOUNTINFO", &declared)])
}

fn hooked(subcommand: &str) -> Result<(World, Ran)> {
    let world = prepared()?;
    let table = table(&world)?;
    let hook = append_script(&world, &table, "hook.sh")?;
    let hook = hook.display().to_string();
    world.configure_double(&[(&format!("HOOK_{subcommand}"), &hook)])?;
    let ran = run_with_table(&world, &job_fixture("mise_action_ok"), &table)?;
    Ok((world, ran))
}

#[test]
fn mise_action_refuses_a_mount_that_appears_while_mise_installs() -> Result<()> {
    let (world, ran) = hooked("install")?;
    assert_failed(&ran, &expected(&world));
    let calls = world.mise_calls();
    ensure!(
        !calls.iter().any(|call| call.starts_with("argv=env ")),
        "mise ran after the mount appeared: {calls:?}"
    );
    Ok(())
}

#[test]
fn mise_action_refuses_a_mount_that_appears_before_it_exports() -> Result<()> {
    let (world, ran) = hooked("env")?;
    assert_failed(&ran, &expected(&world));
    ensure!(!ran.stdout.contains("tooly"), "{}", ran.stdout);
    Ok(())
}

fn with_second_mise_step(world: &World, job: &Path) -> Result<PathBuf> {
    let mut message: Value =
        serde_json::from_slice(&std::fs::read(job).context("read the job")?).context("parse")?;
    let steps = message["Steps"].as_array_mut().context("the job steps")?;
    let mut again = steps[0].clone();
    again["Id"] = "00000000-0000-0000-0000-0000000000f2".into();
    again["Name"] = "mise2".into();
    again["ContextName"] = "mise2".into();
    steps.push(again);
    let patched = world.root.join("two-mise.json");
    std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
    Ok(patched)
}

#[test]
fn mise_action_refuses_a_mount_that_a_step_added_before_the_next_mise_step() -> Result<()> {
    let world = prepared()?;
    let table = table(&world)?;
    let hook = append_script(&world, &table, "step.sh")?;
    let job = world.with_final_script(
        &job_fixture("mise_action_ok"),
        &format!("sh {}", hook.display()),
    )?;
    let job = with_second_mise_step(&world, &job)?;
    let ran = run_with_table(&world, &job, &table)?;
    ensure!(
        ran.has_line(&expected(&world)) && ran.has_line("step 3 failed"),
        "{}",
        ran.stdout
    );
    ensure!(
        world.mise_installs().len() == 1,
        "the second mise step installed: {:?}",
        world.mise_calls()
    );
    Ok(())
}
