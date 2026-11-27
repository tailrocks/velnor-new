// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::exec_support::Ran;
use crate::mise_double::each_row;
use crate::mise_real::canonical;
use crate::mise_store::{Ancestor, StoreMode};
use crate::mise_world::{VERSION, World, assert_failed, job_fixture};

#[derive(Clone, Copy, Debug)]
enum Layout {
    LinkedBinary,
    LinkedVersion,
    LinkedFamily,
    DirectoryBinary,
}

fn relocate(world: &World, from: &Path, name: &str) -> Result<PathBuf> {
    let target = world.root.join("elsewhere").join(name);
    std::fs::create_dir_all(target.parent().context("the target parent")?)
        .context("create the writable place")?;
    std::fs::rename(from, &target).context("move into the writable place")?;
    std::os::unix::fs::symlink(&target, from).context("link back into the store")?;
    Ok(target)
}

fn lay_out(world: &World, layout: Layout) -> Result<()> {
    let family = world.store.join("tools/mise");
    match layout {
        Layout::LinkedBinary => relocate(world, &world.binary_dir().join("mise"), "mise").map(drop),
        Layout::LinkedVersion => relocate(world, &world.binary_dir(), "version").map(drop),
        Layout::LinkedFamily => relocate(world, &family, "family").map(drop),
        Layout::DirectoryBinary => {
            let binary = world.binary_dir().join("mise");
            std::fs::remove_file(&binary).context("remove the binary")?;
            std::fs::create_dir(&binary).context("make a directory of the binary")
        }
    }
}

#[test]
fn mise_action_refuses_a_mise_binary_that_is_not_a_regular_file_in_the_store() -> Result<()> {
    let rows = [
        Layout::LinkedBinary,
        Layout::LinkedVersion,
        Layout::LinkedFamily,
        Layout::DirectoryBinary,
    ];
    each_row(&rows, |layout| {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace(true)?;
        lay_out(&world, layout)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        let why = match layout {
            Layout::DirectoryBinary => "is not a regular file",
            _ => "is reached through a link",
        };
        let binary = world.binary_dir().join("mise");
        assert_failed(
            &ran,
            &format!(
                "mise policy: {} {why}; the tools store holds only real files",
                binary.display()
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{layout:?}");
        Ok(())
    })
}

fn with_entry(prepare: impl FnOnce(&World) -> Result<PathBuf>) -> Result<(World, Ran, String)> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    world.install_store_homes()?;
    let entry = prepare(&world)?.display().to_string();
    world.configure_double(&[("EXTRA_DIR", &entry)])?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), "echo \"path=$PATH\"")?;
    let ran = world.run(&job, &[])?;
    Ok((world, ran, entry))
}

fn outside(world: &World) -> Result<PathBuf> {
    let directory = world.root.join("writable-bin");
    std::fs::create_dir_all(&directory).context("create the writable directory")?;
    Ok(canonical(&directory))
}

fn refused_entry(ran: &Ran, entry: &str, resolved: &Path) {
    assert_failed(
        ran,
        &format!(
            "mise policy: PATH entry {entry} resolves to {} outside the read-only tools store",
            resolved.display()
        ),
    );
    assert!(!ran.stdout.contains("path="), "{}", ran.stdout);
}

#[test]
fn mise_action_refuses_a_path_entry_whose_store_link_leaves_the_store() -> Result<()> {
    let mut resolved = PathBuf::new();
    let (_world, ran, entry) = with_entry(|world| {
        resolved = outside(world)?;
        let link = world.data_dir().join("installs/evil");
        std::os::unix::fs::symlink(&resolved, &link).context("link out of the store")?;
        Ok(link)
    })?;
    refused_entry(&ran, &entry, &resolved);
    Ok(())
}

#[test]
fn mise_action_refuses_a_path_entry_that_climbs_out_of_the_store() -> Result<()> {
    let mut resolved = PathBuf::new();
    let (_world, ran, entry) = with_entry(|world| {
        let directory = world.store.join("outside-bin");
        std::fs::create_dir_all(&directory).context("create the sibling directory")?;
        resolved = canonical(&directory);
        Ok(world.data_dir().join("../../outside-bin"))
    })?;
    refused_entry(&ran, &entry, &resolved);
    Ok(())
}

#[test]
fn mise_action_accepts_a_path_entry_whose_store_link_stays_in_the_store() -> Result<()> {
    let (_world, ran, entry) = with_entry(|world| {
        let installs = world.data_dir().join("installs");
        std::fs::create_dir_all(installs.join("real/bin")).context("create the install")?;
        let link = installs.join("inner");
        std::os::unix::fs::symlink("./real/bin", &link).context("link inside the store")?;
        Ok(link)
    })?;
    ensure!(
        ran.has_line("job succeeded") && ran.stdout.contains(&entry),
        "{entry}\n{}",
        ran.stdout
    );
    Ok(())
}

fn linked_store(world: &World) -> Result<String> {
    world.install_mise()?;
    world.init_workspace(true)?;
    world.install_store_homes()?;
    let link = world.root.join("store-link");
    std::os::unix::fs::symlink(&world.store, &link).context("link the store root")?;
    Ok(link.display().to_string())
}

#[test]
fn mise_action_accepts_a_store_root_reached_through_a_link_to_a_trusted_store() -> Result<()> {
    let world = World::new()?;
    let root = linked_store(&world)?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), "echo \"path=$PATH\"")?;
    let ran = world.run(&job, &[("VELNOR_STORE_ROOT", &root)])?;
    ensure!(ran.has_line("job succeeded"), "{}", ran.stdout);
    ensure!(world.mise_installs().len() == 1, "{:?}", world.mise_calls());
    ensure!(
        !ran.stdout.contains(&root),
        "the link reached a child\n{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn mise_action_refuses_a_store_root_link_whose_target_is_not_trusted() -> Result<()> {
    let mut world = World::new()?;
    world.store_mode = StoreMode::Loose(Ancestor::Store, 0o757);
    let root = linked_store(&world)?;
    let ran = world.run(
        &job_fixture("mise_action_ok"),
        &[("VELNOR_STORE_ROOT", &root)],
    )?;
    assert_failed(
        &ran,
        &format!(
            "mise policy: {} is writable by its group or others (mode 757); no directory above the tools store may be",
            canonical(&world.store).display()
        ),
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

type Row<'a> = (&'a str, &'a str, &'a str);

fn nested(role: &str, root: &str, point: &str) -> String {
    format!(
        "mise policy: {role} {{store}}/tools/{root} holds a writable mount at {{store}}/tools/{point}; the tools store must be one read-only mount"
    )
}

fn table(line: &str) -> String {
    format!("1 0 0:1 / / rw - apfs root rw\n{line}\n")
}

fn attempt(label: &str, line: &str, expected: &str) -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let store = canonical(&world.store).display().to_string();
    let file = world.root.join("mountinfo");
    std::fs::write(&file, table(&line.replace("{store}", &store)))
        .context("write the mount table")?;
    let declared = file.display().to_string();
    let ran = world.run(
        &job_fixture("mise_action_ok"),
        &[("VELNOR_TEST_MOUNTINFO", &declared)],
    )?;
    if expected.is_empty() {
        ensure!(ran.has_line("job succeeded"), "{label}\n{}", ran.stdout);
        return Ok(());
    }
    assert_failed(&ran, &expected.replace("{store}", &store));
    assert_eq!(world.mise_calls(), Vec::<String>::new(), "{label}");
    Ok(())
}

#[test]
fn mise_action_refuses_a_writable_mount_nested_under_the_tools_store() -> Result<()> {
    let data = nested("MISE_DATA_DIR", "data", "data/installs");
    let rustup = nested("MISE_RUSTUP_HOME", "rustup", "rustup/x");
    let cargo = nested("MISE_CARGO_HOME", "cargo", "cargo/x");
    let version = format!("mise/{VERSION}");
    let binary = nested(
        "the mise binary directory",
        &version,
        &format!("{version}/x"),
    );
    let rows: [Row<'_>; 10] = [
        (
            "writable",
            "41 1 0:50 / {store}/tools/data/installs rw - ext4 /dev/b rw",
            &data,
        ),
        (
            "rustup",
            "41 1 0:50 / {store}/tools/rustup/x rw - ext4 /dev/b rw",
            &rustup,
        ),
        (
            "cargo",
            "41 1 0:50 / {store}/tools/cargo/x rw - ext4 /dev/b rw",
            &cargo,
        ),
        (
            "binary directory",
            &format!("41 1 0:50 / {{store}}/tools/{version}/x rw - ext4 /dev/b rw"),
            &binary,
        ),
        (
            "escaped separator",
            "41 1 0:50 / {store}/tools/data\\057installs rw - ext4 /dev/b rw",
            &data,
        ),
        (
            "read-only mount",
            "41 1 0:50 / {store}/tools/data/installs ro - ext4 /dev/b rw",
            "",
        ),
        (
            "read-only superblock",
            "41 1 0:50 / {store}/tools/data/installs rw - ext4 /dev/b ro",
            "",
        ),
        (
            "sibling of the data directory",
            "41 1 0:50 / {store}/tools/data-extra rw - ext4 /dev/b rw",
            "",
        ),
        (
            "outside the store",
            "41 1 0:50 / /elsewhere rw - ext4 /dev/b rw",
            "",
        ),
        (
            "unreadable table",
            "not a mount line",
            "mise policy: the mount table from /proc/self/mountinfo cannot be parsed",
        ),
    ];
    each_row(&rows, |row| attempt(row.0, row.1, row.2))
}

#[cfg(target_os = "linux")]
#[test]
fn mise_action_reads_the_host_mount_table_on_linux() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(
        &job_fixture("mise_action_ok"),
        &[("VELNOR_TEST_MOUNTINFO", "")],
    )?;
    ensure!(ran.has_line("job succeeded"), "{}", ran.stdout);
    Ok(())
}
