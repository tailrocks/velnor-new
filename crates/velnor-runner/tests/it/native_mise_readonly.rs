// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::mise_double::each_row;
use crate::mise_real::{RealMise, canonical, real_mise};
use crate::mise_repo::{REPO_MISE_LOCK, repo_config_without_wrappers};
use crate::mise_store::{Dir, StoreMode};
use crate::mise_world::{World, assert_failed, job_fixture};

const SCRIPT: &str = "echo \"$PATH\" | tr ':' '\\n' | sed 's/^/pathentry=/'";
const READ_ONLY: &str = "is not on a read-only mount; the tools store must be mounted read-only";

#[derive(Deserialize)]
struct Installed {
    install_path: String,
}

fn preinstall(world: &World, real: &RealMise, skip: Option<&str>) -> Result<()> {
    let data = world.data_dir();
    let tools = world.store.join("tools");
    let env = vec![
        ("MISE_DATA_DIR".to_owned(), data.display().to_string()),
        (
            "MISE_CARGO_HOME".to_owned(),
            tools.join("cargo").display().to_string(),
        ),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            tools.join("rustup").display().to_string(),
        ),
    ];
    let listing = world
        .control(real, &world.workspace(), &env)
        .args(["ls", "--json"])
        .output()
        .context("list the tools")?;
    ensure!(
        listing.status.success(),
        "mise ls failed ({})",
        real.version
    );
    let listed: std::collections::BTreeMap<String, Vec<Installed>> =
        serde_json::from_slice(&listing.stdout).context("parse the tool listing")?;
    for (name, versions) in &listed {
        for version in versions {
            let path = Path::new(&version.install_path);
            if name == "rust" {
                std::fs::create_dir_all(path.parent().context("the rust install parent")?)
                    .context("create the rust installs")?;
                std::os::unix::fs::symlink(tools.join("cargo/bin"), path)
                    .context("link the rust install")?;
            } else {
                std::fs::create_dir_all(path.join("bin")).context("create the install")?;
            }
        }
    }
    let installed = world
        .control(real, &world.workspace(), &env)
        .args(["install", "--locked"])
        .output()
        .context("run the store build")?;
    ensure!(
        installed.status.success(),
        "the store build failed ({}): {}",
        real.version,
        String::from_utf8_lossy(&installed.stderr)
    );
    for (name, versions) in &listed {
        if skip.is_some_and(|skipped| name.contains(skipped)) {
            for version in versions {
                let parent = Path::new(&version.install_path).parent();
                std::fs::remove_dir_all(parent.context("the install parent")?)
                    .context("remove the skipped tool")?;
            }
        }
    }
    Ok(())
}

fn repository_world(real: &RealMise, skip: Option<&str>) -> Result<World> {
    let world = World::new()?;
    world.install_real_mise(real)?;
    world.init_workspace_with_lock(&repo_config_without_wrappers()?, REPO_MISE_LOCK)?;
    world.install_store_homes()?;
    std::fs::remove_dir_all(world.data_dir().join("installs/rust"))
        .context("remove the fixture rust installs")?;
    preinstall(&world, real, skip)?;
    Ok(world)
}

fn snapshot(root: &Path, directory: &Path, found: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        let target = std::fs::read_link(&path).map_or_default(|link| link.display().to_string());
        let name = path.strip_prefix(root)?.display();
        found.push(format!("{name} {} {target}", meta.len()));
        if meta.is_dir() {
            snapshot(root, &path, found)?;
        }
    }
    found.sort();
    Ok(())
}

fn store_tree(world: &World) -> Result<Vec<String>> {
    let mut found = Vec::new();
    for directory in [
        world.data_dir(),
        world.rustup_dir(),
        world.cargo_dir(),
        world.binary_dir(),
    ] {
        snapshot(&directory, &directory, &mut found)?;
    }
    Ok(found)
}

#[test]
fn mise_action_runs_the_repository_config_against_a_read_only_store() -> Result<()> {
    let real = real_mise(&World::new()?)?;
    let world = repository_world(&real, None)?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), SCRIPT)?;
    let data = world.data_dir();
    world.seal_store()?;
    let before = store_tree(&world)?;
    let ran = World::finish(world.start_clean(&job, &[])?)?;
    ensure!(
        ran.has_line("job succeeded") && ran.code == Some(0),
        "({}) {}",
        real.version,
        ran.stdout
    );
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
    let after = store_tree(&world)?;
    ensure!(
        after == before,
        "the job changed the tools store: before only {:?}, after only {:?}",
        before
            .iter()
            .filter(|line| !after.contains(line))
            .collect::<Vec<_>>(),
        after
            .iter()
            .filter(|line| !before.contains(line))
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn mise_action_fails_the_repository_config_when_a_tool_is_missing_from_the_store() -> Result<()> {
    let real = real_mise(&World::new()?)?;
    let world = repository_world(&real, Some("shellcheck"))?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), SCRIPT)?;
    let ran = World::finish(world.start_clean(&job, &[])?)?;
    assert_failed(&ran, "tool is not installed in the tools store");
    assert!(ran.stdout.contains("Permission denied"), "{}", ran.stdout);
    let installs = world.data_dir().join("installs");
    let installed: Vec<String> = std::fs::read_dir(installs)?
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .filter(|name| name.contains("shellcheck"))
        .collect();
    ensure!(installed.is_empty(), "mise installed {installed:?}");
    Ok(())
}

fn run_with(mode: StoreMode) -> Result<(World, crate::exec_support::Ran)> {
    let mut world = World::new()?;
    world.store_mode = mode;
    world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    Ok((world, ran))
}

#[test]
fn mise_action_refuses_a_store_directory_that_is_not_on_a_read_only_mount() -> Result<()> {
    each_row(&[Dir::Data, Dir::Rustup, Dir::Cargo, Dir::Binary], |dir| {
        let (world, ran) = run_with(StoreMode::Writable(dir))?;
        assert_failed(
            &ran,
            &format!(
                "mise policy: {} {} {READ_ONLY}",
                dir.role(),
                world.dir(dir).display()
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{dir:?}");
        Ok(())
    })
}

#[test]
fn mise_action_does_not_take_a_mode_bit_sealed_store_for_a_read_only_mount() -> Result<()> {
    each_row(&[StoreMode::Real, StoreMode::Bits], |mode| {
        let (world, ran) = run_with(mode)?;
        assert_failed(
            &ran,
            &format!(
                "mise policy: MISE_DATA_DIR {} {READ_ONLY}",
                world.data_dir().display()
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new());
        let installs = std::fs::read_dir(world.data_dir().join("installs"))?;
        assert_eq!(installs.count(), 0);
        Ok(())
    })
}

#[test]
fn mise_action_refuses_a_directory_missing_from_the_store() -> Result<()> {
    each_row(&[Dir::Data, Dir::Rustup, Dir::Cargo], |dir| {
        let (world, ran) = run_with(StoreMode::Missing(dir))?;
        assert_failed(
            &ran,
            &format!(
                "mise policy: {} {} is missing from the tools store",
                dir.role(),
                world.dir(dir).display()
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{dir:?}");
        Ok(())
    })
}

#[test]
fn mise_action_refuses_a_store_directory_that_is_a_link() -> Result<()> {
    each_row(&[Dir::Data, Dir::Rustup, Dir::Cargo], |dir| {
        let (world, ran) = run_with(StoreMode::Linked(dir))?;
        assert_failed(
            &ran,
            &format!(
                "mise policy: {} {} is reached through a link; the tools store holds only real directories",
                dir.role(),
                world.dir(dir).display()
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{dir:?}");
        Ok(())
    })
}
