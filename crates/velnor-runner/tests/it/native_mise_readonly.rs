// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::mise_real::{RealMise, canonical, real_mise};
use crate::mise_repo::{REPO_MISE_LOCK, repo_config_without_wrappers};
use crate::mise_world::{World, job_fixture};

const SCRIPT: &str = "echo \"$PATH\" | tr ':' '\\n' | sed 's/^/pathentry=/'";

#[derive(Deserialize)]
struct Installed {
    install_path: String,
}

fn preinstall(world: &World, real: &RealMise) -> Result<()> {
    let data = world.data_dir();
    let env = vec![("MISE_DATA_DIR".to_owned(), data.display().to_string())];
    let output = world
        .control(real, &world.workspace(), &env)
        .args(["ls", "--json"])
        .output()
        .context("list the tools")?;
    ensure!(output.status.success(), "mise ls failed ({})", real.version);
    let tools: std::collections::BTreeMap<String, Vec<Installed>> =
        serde_json::from_slice(&output.stdout).context("parse the tool listing")?;
    for (name, versions) in tools {
        for version in versions {
            let path = Path::new(&version.install_path);
            if path.symlink_metadata().is_ok() {
                continue;
            }
            if name == "rust" {
                std::fs::create_dir_all(path.parent().context("the rust install parent")?)
                    .context("create the rust installs")?;
                std::os::unix::fs::symlink(world.store.join("tools/cargo/bin"), path)
                    .context("link the rust install")?;
            } else {
                std::fs::create_dir_all(path.join("bin")).context("create the install")?;
            }
        }
    }
    Ok(())
}

#[test]
fn mise_action_installs_the_repository_config_into_a_read_only_data_dir() -> Result<()> {
    let world = World::new()?;
    let real = real_mise(&world)?;
    world.install_real_mise(&real)?;
    world.init_workspace_with_lock(&repo_config_without_wrappers()?, REPO_MISE_LOCK)?;
    world.install_store_homes()?;
    preinstall(&world, &real)?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), SCRIPT)?;
    let ran = World::finish(world.start_clean(&job, &[])?)?;
    ensure!(
        ran.has_line("job succeeded") && ran.code == Some(0),
        "({}) {}",
        real.version,
        ran.stdout
    );
    let data = world.data_dir();
    let node = data.join("installs/node/26.13.1/bin");
    ensure!(
        ran.lines().into_iter().any(|line| line
            .strip_prefix("pathentry=")
            .is_some_and(|entry| canonical(Path::new(entry)) == canonical(&node))),
        "the node install is not on the later PATH:\n{}",
        ran.stdout
    );
    let mode = std::fs::metadata(&data)?.permissions().mode();
    ensure!(
        mode & 0o222 == 0,
        "the data directory is writable: {mode:o}"
    );
    for name in ["command-wrappers", "shims"] {
        ensure!(
            !data.join(name).exists(),
            "mise wrote {name} into the store"
        );
    }
    Ok(())
}
