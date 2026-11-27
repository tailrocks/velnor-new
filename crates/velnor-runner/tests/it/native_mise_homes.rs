// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Result, ensure};

use crate::mise_attack_state::{RUST_CONFIG, RUST_LOCK};
use crate::mise_double::each_row;
use crate::mise_real::{canonical, real_mise};
use crate::mise_world::{World, job_fixture};

const SCRIPT: &str = "echo \"cargo_home=${CARGO_HOME-unset}\"\necho \"rustup_home=$RUSTUP_HOME\"\necho \"toolchain=$RUSTUP_TOOLCHAIN\"\necho \"mise_cargo_home=$MISE_CARGO_HOME\"\necho \"mise_rustup_home=$MISE_RUSTUP_HOME\"";

fn value(stdout: &str, name: &str) -> String {
    let prefix = format!("{name}=");
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map_or_default(str::to_owned)
}

#[test]
fn mise_action_leaves_cargo_home_to_the_job_and_pins_mises_homes() -> Result<()> {
    each_row(&["process env", "job message env", "none"], |row| {
        let world = World::new()?;
        let real = real_mise(&world)?;
        world.install_real_mise(&real)?;
        world.init_workspace_with_lock(RUST_CONFIG, RUST_LOCK)?;
        world.install_store_homes()?;
        let mut job = world.with_final_script(&job_fixture("mise_action_ok"), SCRIPT)?;
        let mut process_env = Vec::new();
        match row {
            "process env" => process_env.push(("CARGO_HOME", "/c")),
            "job message env" => job = world.with_job_env(&job, &[("CARGO_HOME", "/c")])?,
            _ => {}
        }
        let ran = World::finish(world.start_clean(&job, &process_env)?)?;
        ensure!(ran.has_line("job succeeded"), "{}", ran.stdout);
        let tools = world.store.join("tools");
        let wanted_cargo = if row == "none" { "unset" } else { "/c" };
        ensure!(
            value(&ran.stdout, "cargo_home") == wanted_cargo,
            "CARGO_HOME of the later step:\n{}",
            ran.stdout
        );
        for (name, home) in [("mise_cargo_home", "cargo"), ("mise_rustup_home", "rustup")] {
            ensure!(
                canonical(std::path::Path::new(&value(&ran.stdout, name)))
                    == canonical(&tools.join(home)),
                "{name}:\n{}",
                ran.stdout
            );
        }
        ensure!(
            canonical(std::path::Path::new(&value(&ran.stdout, "rustup_home")))
                == canonical(&tools.join("rustup"))
                && value(&ran.stdout, "toolchain") == "1.90.0",
            "RUSTUP_HOME and RUSTUP_TOOLCHAIN:\n{}",
            ran.stdout
        );
        Ok(())
    })
}
