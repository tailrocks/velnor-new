// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::mise_real::{ACCEPTED_CONFIG, real_mise};
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
        "-size".as_ref(),
        "32m".as_ref(),
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

#[test]
fn mise_action_accepts_a_store_on_a_read_only_mount() -> Result<()> {
    let world = World::new()?;
    let real = real_mise(&world)?;
    world.install_real_mise(&real)?;
    world.init_workspace_with_lock(ACCEPTED_CONFIG, "")?;
    world.install_store_homes()?;
    let script = "echo \"accepted=$ACCEPTED\"";
    let job = world.with_final_script(&job_fixture("mise_action_ok"), script)?;
    let mut command = world.command(&job)?;
    let mounted = mount_read_only(&world)?;
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", world.home())
        .env("VELNOR_STORE_ROOT", &mounted.mountpoint)
        .env("MISE_CEILING_PATHS", world.ceiling());
    let ran = World::finish(command.spawn().context("start velnor-runner exec")?)?;
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
