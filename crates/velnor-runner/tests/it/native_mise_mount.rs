// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::exec_support::Ran;
use crate::mise_real::{ACCEPTED_CONFIG, RealMise, canonical, real_mise};
use crate::mise_world::{VERSION, World, job_fixture};

struct Mounted {
    mountpoint: PathBuf,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(&self.mountpoint)
            .output();
    }
}

fn hdiutil(args: &[&std::ffi::OsStr]) -> Result<()> {
    let output = Command::new("hdiutil")
        .args(args)
        .output()
        .context("run hdiutil")?;
    ensure!(
        output.status.success(),
        "hdiutil {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn mount_read_only(world: &World) -> Result<Mounted> {
    let image = world.root.join("store.dmg");
    let mountpoint = world.root.join("mnt");
    std::fs::create_dir_all(&mountpoint).context("create the mount point")?;
    hdiutil(&[
        "create".as_ref(),
        "-fs".as_ref(),
        "HFS+".as_ref(),
        "-volname".as_ref(),
        "velnorstore".as_ref(),
        "-srcfolder".as_ref(),
        world.store.as_os_str(),
        image.as_os_str(),
    ])?;
    let mounted = Mounted { mountpoint };
    hdiutil(&[
        "attach".as_ref(),
        "-readonly".as_ref(),
        "-nobrowse".as_ref(),
        "-noverify".as_ref(),
        "-mountpoint".as_ref(),
        mounted.mountpoint.as_os_str(),
        image.as_os_str(),
    ])?;
    Ok(mounted)
}

fn run_on_mount(world: &World, mut command: Command, mounted: &Mounted) -> Result<Ran> {
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", world.home())
        .env("VELNOR_STORE_ROOT", &mounted.mountpoint)
        .env("MISE_CEILING_PATHS", world.ceiling())
        .env("VELNOR_TEST_STORE_TRUST", world.trust_declaration());
    World::finish(command.spawn().context("start velnor-runner exec")?)
}

fn prepared() -> Result<(World, RealMise, Command)> {
    let world = World::new()?;
    let real = real_mise(&world)?;
    world.install_real_mise(&real)?;
    world.init_workspace_with_lock(ACCEPTED_CONFIG, "")?;
    world.install_store_homes()?;
    let script = "echo \"accepted=$ACCEPTED\"";
    let job = world.with_final_script(&job_fixture("mise_action_ok"), script)?;
    let command = world.command(&job)?;
    Ok((world, real, command))
}

#[test]
fn mise_action_accepts_a_store_on_a_read_only_mount() -> Result<()> {
    let (world, real, command) = prepared()?;
    let mounted = mount_read_only(&world)?;
    let ran = run_on_mount(&world, command, &mounted)?;
    ensure!(
        ran.has_line("accepted=1") && ran.has_line("job succeeded"),
        "({}) {}",
        real.version,
        ran.stdout
    );
    ensure!(
        std::fs::create_dir(mounted.mountpoint.join("tools/data/probe")).is_err(),
        "the mount is writable"
    );
    Ok(())
}

fn attach_writable(world: &World, mountpoint: PathBuf) -> Result<Mounted> {
    let image = world.root.join("nested.dmg");
    hdiutil(&[
        "create".as_ref(),
        "-size".as_ref(),
        "8m".as_ref(),
        "-fs".as_ref(),
        "HFS+".as_ref(),
        "-volname".as_ref(),
        "nestedrw".as_ref(),
        image.as_os_str(),
    ])?;
    let mounted = Mounted { mountpoint };
    hdiutil(&[
        "attach".as_ref(),
        "-nobrowse".as_ref(),
        "-noverify".as_ref(),
        "-mountpoint".as_ref(),
        mounted.mountpoint.as_os_str(),
        image.as_os_str(),
    ])?;
    Ok(mounted)
}

#[derive(Debug)]
struct Nested {
    role: &'static str,
    root: String,
    point: String,
}

fn nested_rows() -> Vec<Nested> {
    let binary = format!("tools/mise/{VERSION}");
    let row = |role, root: &str, point: &str| Nested {
        role,
        root: root.to_owned(),
        point: point.to_owned(),
    };
    vec![
        row(
            "MISE_DATA_DIR",
            "tools/data",
            "tools/data/installs/shellcheck/0.11.0",
        ),
        row(
            "MISE_RUSTUP_HOME",
            "tools/rustup",
            "tools/rustup/toolchains",
        ),
        row(
            "the mise binary directory",
            &binary,
            &format!("{binary}/lib"),
        ),
    ]
}

fn refuses_nested(row: &Nested) -> Result<()> {
    let (world, real, command) = prepared()?;
    std::fs::create_dir_all(world.store.join(&row.point)).context("create the mount point")?;
    let mounted = mount_read_only(&world)?;
    let nested = attach_writable(&world, mounted.mountpoint.join(&row.point))?;
    let ran = run_on_mount(&world, command, &mounted)?;
    let base = canonical(&mounted.mountpoint);
    let expected = format!(
        "mise policy: {} {} holds a writable mount at {}; the tools store must be one read-only mount",
        row.role,
        base.join(&row.root).display(),
        base.join(&row.point).display()
    );
    ensure!(
        ran.has_line(&expected) && ran.has_line("job failed") && !ran.has_line("job succeeded"),
        "({}) {expected}\n{}",
        real.version,
        ran.stdout
    );
    ensure!(
        !nested.mountpoint.join("shellcheck").exists(),
        "mise wrote into the nested mount"
    );
    Ok(())
}

#[test]
fn mise_action_refuses_a_writable_mount_nested_under_a_read_only_store() -> Result<()> {
    let failed: Vec<String> = nested_rows()
        .iter()
        .filter_map(|row| Some(format!("row {row:?}: {:#}", refuses_nested(row).err()?)))
        .collect();
    ensure!(failed.is_empty(), "{}", failed.join("\n"));
    Ok(())
}
