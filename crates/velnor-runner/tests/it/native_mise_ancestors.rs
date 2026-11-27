// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Result, ensure};

use crate::mise_double::each_row;
use crate::mise_real::canonical;
use crate::mise_store::{Ancestor, StoreMode};
use crate::mise_world::{World, assert_failed, job_fixture};

#[derive(Clone, Copy, Debug)]
struct Row {
    ancestor: Ancestor,
    mode: u32,
}

fn world_with(mode: StoreMode) -> Result<World> {
    let mut world = World::new()?;
    world.store_mode = mode;
    world.install_mise()?;
    world.init_workspace(true)?;
    Ok(world)
}

fn path_of(world: &World, ancestor: Ancestor) -> std::path::PathBuf {
    canonical(&match ancestor {
        Ancestor::Root => world.root.clone(),
        Ancestor::Store => world.store.clone(),
        Ancestor::Tools => world.store.join("tools"),
    })
}

fn refused_as_writable(row: Row) -> Result<()> {
    let world = world_with(StoreMode::Loose(row.ancestor, row.mode))?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(
        &ran,
        &format!(
            "mise policy: {} is writable by its group or others (mode {:o}); no directory above the tools store may be",
            path_of(&world, row.ancestor).display(),
            row.mode
        ),
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new(), "{row:?}");
    Ok(())
}

#[test]
fn mise_action_refuses_a_tools_store_below_a_directory_others_can_write() -> Result<()> {
    let rows = [
        Row {
            ancestor: Ancestor::Root,
            mode: 0o775,
        },
        Row {
            ancestor: Ancestor::Store,
            mode: 0o757,
        },
        Row {
            ancestor: Ancestor::Tools,
            mode: 0o775,
        },
        Row {
            ancestor: Ancestor::Tools,
            mode: 0o757,
        },
    ];
    each_row(&rows, refused_as_writable)
}

#[test]
fn mise_action_refuses_a_tools_store_below_a_directory_of_another_owner() -> Result<()> {
    let world = world_with(StoreMode::Sealed)?;
    let owner = world.trust_declaration();
    let (uid, anchor) = owner.split_once(':').unwrap_or_default();
    let other = uid.parse::<u32>()? + 1;
    let declared = format!("{other}:{anchor}");
    let ran = world.run(
        &job_fixture("mise_action_ok"),
        &[("VELNOR_TEST_STORE_TRUST", &declared)],
    )?;
    assert_failed(
        &ran,
        &format!(
            "mise policy: {} is owned by uid {uid}; every directory above the tools store must be owned by uid {other}",
            Path::new(anchor).display()
        ),
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_does_not_examine_a_directory_above_its_trust_anchor() -> Result<()> {
    let world = world_with(StoreMode::Loose(Ancestor::Root, 0o777))?;
    let owner = world.trust_declaration();
    let (uid, _) = owner.split_once(':').unwrap_or_default();
    let declared = format!("{uid}:{}", canonical(&world.store).display());
    let ran = world.run(
        &job_fixture("mise_action_ok"),
        &[("VELNOR_TEST_STORE_TRUST", &declared)],
    )?;
    ensure!(ran.has_line("job succeeded"), "{}", ran.stdout);
    Ok(())
}
