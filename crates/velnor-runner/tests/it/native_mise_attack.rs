// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::mise_real::{Plant, Row, attack, real_mise};
use crate::mise_world::{World, job_fixture};

const ROW_IDS: [&str; 10] = ["W1", "W2", "H", "V1", "V2", "V3", "V4", "V5", "V7", "V8"];

fn marker(world: &World, label: &str) -> PathBuf {
    world.root.join("markers").join(label)
}

fn exec(world: &World, row: &str, relative: &str, base: &Path) -> Plant {
    let label = format!("{row}-{}", relative.replace('/', "_"));
    Plant::exec(base.join(relative), marker(world, &label))
}

fn file_row(world: &World, id: &str) -> Option<(Vec<Plant>, Option<PathBuf>)> {
    let workspace = world.workspace();
    let root = world.root.clone();
    let home = world.home();
    let temp = world.temp();
    Some(match id {
        "W1" => (
            vec![
                exec(world, id, "mise.local.toml", &workspace),
                Plant::plain(
                    workspace.join(".miserc.toml"),
                    "env = [\"staging\"]\n",
                    false,
                ),
                exec(world, id, "mise.staging.toml", &workspace),
                exec(world, id, ".mise/conf.d/a.toml", &workspace),
                exec(world, id, ".config/mise/config.toml", &workspace),
                Plant::plain(workspace.join(".tool-versions"), "node 22.0.0\n", true),
                exec(world, id, "mise.toml", &root),
            ],
            None,
        ),
        "W2" => (
            vec![
                exec(world, id, "mise.toml", &temp),
                Plant::plain(
                    temp.join(".miserc.toml"),
                    "env = [\"staging\"]\nceiling_paths = []\n",
                    false,
                ),
                exec(world, id, "mise.staging.toml", &temp),
                exec(world, id, "mise.toml", &root),
            ],
            Some(temp.join("probe/config")),
        ),
        "H" => (
            vec![
                exec(world, id, ".config/mise/config.toml", &home),
                exec(world, id, ".config/mise/conf.d/x.toml", &home),
            ],
            None,
        ),
        _ => return None,
    })
}

fn variable(name: &str, value: String) -> Vec<(String, String)> {
    vec![(name.to_owned(), value)]
}

type EnvRow = (Vec<Plant>, Vec<(String, String)>);

fn env_row(world: &World, id: &str) -> Option<EnvRow> {
    let workspace = world.workspace();
    let root = world.root.clone();
    let located = |name: &str| root.join(name).to_string_lossy().into_owned();
    Some(match id {
        "V1" => (
            vec![exec(world, id, "mise/config.toml", &root.join("xdg"))],
            variable("XDG_CONFIG_HOME", located("xdg")),
        ),
        "V2" => (
            vec![exec(world, id, "config.toml", &root.join("cfgdir"))],
            variable("MISE_CONFIG_DIR", located("cfgdir")),
        ),
        "V3" | "V4" | "V5" => {
            let name = match id {
                "V3" => "MISE_GLOBAL_CONFIG_FILE",
                "V4" => "MISE_CONFIG_FILE",
                _ => "MISE_SYSTEM_CONFIG_FILE",
            };
            let plant = root.join(format!("plant-{id}.toml"));
            let value = plant.to_string_lossy().into_owned();
            (
                vec![Plant::exec(plant, marker(world, id))],
                variable(name, value),
            )
        }
        "V7" => (
            vec![exec(world, id, "evil.toml", &workspace)],
            variable("MISE_OVERRIDE_CONFIG_FILENAMES", "evil.toml".to_owned()),
        ),
        "V8" => (
            vec![exec(world, id, "mise.toml", &root.join("evil-dir"))],
            variable("MISE_CD", located("evil-dir")),
        ),
        _ => return None,
    })
}

fn row(world: &World, id: &'static str) -> Result<Row> {
    if let Some((plants, control_cwd)) = file_row(world, id) {
        return Ok(Row {
            id,
            plants,
            env: Vec::new(),
            control_cwd,
        });
    }
    let (plants, env) = env_row(world, id).with_context(|| format!("unknown row {id}"))?;
    Ok(Row {
        id,
        plants,
        env,
        control_cwd: None,
    })
}

#[test]
fn mise_action_loads_only_the_private_copy() -> Result<()> {
    let mut failures = Vec::new();
    for id in ROW_IDS {
        let world = World::new()?;
        let real = real_mise(&world)?;
        let row = row(&world, id)?;
        failures.extend(attack(&world, &real, &row)?);
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

const REWRITE_CONFIG: &str = "[settings]\nlockfile = true\n\n[env]\nACCEPTED = \"1\"\nWSV = \"{{config_root}}/val\"\n_.path = [\"bin\", \"{{config_root}}/tbin\"]\n";
const REWRITE_SCRIPT: &str =
    "env\necho \"$PATH\" | tr ':' '\\n' | sed 's/^/pathentry=/'\necho \"wsv=$WSV\"";

#[test]
fn mise_action_rewrites_private_paths_for_real_mise() -> Result<()> {
    let world = World::new()?;
    let real = real_mise(&world)?;
    world.install_real_mise(&real)?;
    world.init_workspace_with_lock(REWRITE_CONFIG, "")?;
    let workspace = world.workspace();
    let canonical = std::fs::canonicalize(&workspace).context("resolve the workspace")?;
    let control = world
        .control(&real, &workspace, &[])
        .args(["env", "--paths"])
        .output()
        .context("run the control")?;
    let paths = String::from_utf8_lossy(&control.stdout).into_owned();
    ensure!(
        control.status.success()
            && paths
                .lines()
                .any(|line| line == format!("{}/bin", canonical.display()))
            && paths
                .lines()
                .any(|line| line == format!("{}/tbin", canonical.display())),
        "the control ({}) did not resolve against the config root: {paths}",
        real.version
    );
    let job = world.with_final_script(&job_fixture("mise_action_ok"), REWRITE_SCRIPT)?;
    let ran = World::finish(world.start_clean(&job, &[])?)?;
    let bin = format!("pathentry={}/bin", workspace.display());
    let tbin = format!("pathentry={}/tbin", workspace.display());
    let shims = format!("pathentry={}/mise-shims", world.temp().display());
    let lines = ran.lines();
    let position = |wanted: &str| lines.iter().position(|line| *line == wanted);
    let ordered = matches!(
        (position(&bin), position(&tbin), position(&shims)),
        (Some(first), Some(second), Some(third)) if first < second && second < third
    );
    ensure!(
        ordered
            && ran.has_line(&format!("wsv={}/val", workspace.display()))
            && !ran.stdout.contains("velnor-mise-")
            && ran.has_line("job succeeded")
            && ran.code == Some(0),
        "({}) bin, tbin and shims in order, WSV rewritten, no private root:\n{}",
        real.version,
        ran.stdout
    );
    Ok(())
}
