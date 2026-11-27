// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::exec_support::Ran;
use crate::mise_real::{ACCEPTED_CONFIG, RealMise, canonical, real_mise};
use crate::mise_world::{World, job_fixture};

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
        .env("MISE_CEILING_PATHS", world.ceiling());
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

#[test]
fn mise_action_refuses_a_writable_mount_nested_under_a_read_only_store() -> Result<()> {
    let (world, real, command) = prepared()?;
    let mounted = mount_read_only(&world)?;
    let nested = attach_writable(&world, mounted.mountpoint.join("tools/data/installs"))?;
    let ran = run_on_mount(&world, command, &mounted)?;
    let data = canonical(&mounted.mountpoint).join("tools/data");
    let expected = format!(
        "mise policy: MISE_DATA_DIR {} holds a writable mount at {}/installs; the tools store must be one read-only mount",
        data.display(),
        data.display()
    );
    ensure!(
        ran.has_line(&expected) && ran.has_line("job failed") && !ran.has_line("job succeeded"),
        "({}) {}",
        real.version,
        ran.stdout
    );
    let planted = nested.mountpoint.join("shellcheck");
    ensure!(!planted.exists(), "mise wrote into the nested mount");
    Ok(())
}
