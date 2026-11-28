// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, ensure};

use crate::mise_real::{ACCEPTED_CONFIG, Plant, real_mise};
use crate::mise_world::{World, assert_failed, job_fixture};

const INCLUDE: &str = "include = [\"../../evil.toml\"]\n\n";

#[test]
fn mise_action_refuses_a_committed_include_of_a_job_written_file() -> Result<()> {
    let world = World::new()?;
    let real = real_mise(&world)?;
    world.install_real_mise(&real)?;
    let config = format!("{INCLUDE}{ACCEPTED_CONFIG}");
    world.init_workspace_with_lock(&config, "")?;
    let marker = world.root.join("markers/include");
    let evil = Plant::exec(world.temp().join("evil.toml"), marker.clone());
    std::fs::create_dir_all(marker.parent().context("the marker directory")?)
        .context("create the marker directory")?;
    std::fs::create_dir_all(world.temp()).context("create the temp directory")?;
    std::fs::write(&evil.path, &evil.content).context("write the job's file")?;
    let control = world.temp().join("control/config");
    std::fs::create_dir_all(&control).context("create the control directory")?;
    std::fs::write(control.join("mise.toml"), &config).context("write the control config")?;
    let loaded = world
        .control(&real, &control, &[])
        .args(["config", "ls", "--json"])
        .output()
        .context("run the control")?;
    ensure!(
        loaded.status.success() && marker.exists(),
        "the control ({}) did not run the included file: {}",
        real.version,
        String::from_utf8_lossy(&loaded.stderr)
    );
    std::fs::remove_file(&marker).context("reset the marker")?;
    let ran = World::finish(world.start_clean(&job_fixture("mise_action_ok"), &[])?)?;
    ensure!(!marker.exists(), "the included file ran:\n{}", ran.stdout);
    assert_failed(
        &ran,
        "unsupported: mise policy: mise.toml key 'include' is not supported by velnor-runner",
    );
    Ok(())
}
