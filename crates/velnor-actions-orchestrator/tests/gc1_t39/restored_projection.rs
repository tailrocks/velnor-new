// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::time::Duration;

use velnor_actions_mise::CheckDeadline;

use super::{fail, overlay_map, qualified_at, restored_roots, stage_command};

pub(super) fn check_restored_child_consumes_verified_state() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let cwd = temp.path().join("repo");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&cwd)?;
    let mut qualified = qualified_at(&home, &cwd)?;
    if !qualified.restored_overlay().is_empty() {
        return Err(fail("fresh check carries a restored overlay"));
    }
    let (state, cargo, rustup, tools) = restored_roots(temp.path());
    qualified.project_restored_homes(&state, &cargo, &rustup, &tools)?;
    let overlay = overlay_map(&qualified);
    if overlay.len() != 4
        || overlay.get(OsStr::new("CARGO_HOME")) != Some(&cargo.as_os_str().to_owned())
        || overlay.get(OsStr::new("RUSTUP_HOME")) != Some(&rustup.as_os_str().to_owned())
    {
        return Err(fail("projected overlay missed restored homes"));
    }
    qualified.project_restored_homes(&state, &cargo, &rustup, &tools)?;
    if overlay_map(&qualified) != overlay {
        return Err(fail("repeated projection drifted"));
    }
    stage_command(&qualified, &home)?;
    let on_disk = fs::read_to_string(home.join("tasks.toml"))?;
    if on_disk != qualified.bound_projection()? {
        return Err(fail("projection changed declared task bytes"));
    }
    let deadline = CheckDeadline::after(Duration::from_secs(120))?;
    let command = qualified.command(deadline)?;
    if command.cwd() != Some(&cwd) {
        return Err(fail("child command moved cwd"));
    }
    if command.argv().last() != Some(&OsString::from("lint")) {
        return Err(fail("child command lost task bytes"));
    }
    let full: BTreeMap<OsString, OsString> = command.full_env().into_iter().collect();
    if full.get(OsStr::new("CARGO_HOME")) != Some(&cargo.as_os_str().to_owned())
        || full.get(OsStr::new("RUSTUP_HOME")) != Some(&rustup.as_os_str().to_owned())
    {
        return Err(fail("child command missed restored homes"));
    }
    if full.get(OsStr::new("HOME")) != Some(&home.as_os_str().to_owned()) {
        return Err(fail("child command left its private home"));
    }
    for lane in ["build", "clippy", "doc"] {
        let target = qualified.lane_target_dir(&state, lane)?;
        let build = qualified.lane_build_dir(&state, lane)?;
        if target != state.join(lane).join("target") || build != state.join(lane).join("build") {
            return Err(fail("lane dirs drifted outside restored state"));
        }
        if target == home
            || build == home
            || target == cwd
            || home.starts_with(&target)
            || cwd.starts_with(&build)
        {
            return Err(fail("lane dirs overlap private homes"));
        }
    }
    Ok(())
}

pub(super) fn check_unprojected_child_keeps_private_homes() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let cwd = temp.path().join("repo");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&cwd)?;
    let qualified = qualified_at(&home, &cwd)?;
    if !qualified.restored_overlay().is_empty() {
        return Err(fail("fresh check carries a restored overlay"));
    }
    stage_command(&qualified, &home)?;
    let deadline = CheckDeadline::after(Duration::from_secs(120))?;
    let full: BTreeMap<OsString, OsString> = qualified
        .command(deadline)?
        .full_env()
        .into_iter()
        .collect();
    if full.get(OsStr::new("CARGO_HOME")) != Some(&home.join("cargo").as_os_str().to_owned()) {
        return Err(fail("unprojected child left its private cargo home"));
    }
    Ok(())
}

pub(super) fn check_projection_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let cwd = temp.path().join("repo");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&cwd)?;
    let (state, cargo, rustup, tools) = restored_roots(temp.path());
    let nested = state.join("nested");
    let mut qualified = qualified_at(&home, &cwd)?;
    for roots in [
        (
            Path::new("owned/state"),
            cargo.as_path(),
            rustup.as_path(),
            tools.as_path(),
        ),
        (
            state.as_path(),
            Path::new("owned/cargo"),
            rustup.as_path(),
            tools.as_path(),
        ),
        (
            home.as_path(),
            cargo.as_path(),
            rustup.as_path(),
            tools.as_path(),
        ),
        (
            state.as_path(),
            home.as_path(),
            rustup.as_path(),
            tools.as_path(),
        ),
        (
            state.as_path(),
            cargo.as_path(),
            rustup.as_path(),
            cargo.as_path(),
        ),
        (
            state.as_path(),
            cargo.as_path(),
            rustup.as_path(),
            nested.as_path(),
        ),
        (
            cwd.as_path(),
            cargo.as_path(),
            rustup.as_path(),
            tools.as_path(),
        ),
    ] {
        let (state_root, cargo_home, rustup_home, tool_root) = roots;
        if qualified
            .project_restored_homes(state_root, cargo_home, rustup_home, tool_root)
            .is_ok()
        {
            return Err(fail("projection accepted invalid restored state"));
        }
        if !qualified.restored_overlay().is_empty() {
            return Err(fail("rejected projection mutated the check"));
        }
    }
    for lane in ["", ".", "..", "a/b", "a\\b"] {
        if qualified.lane_target_dir(&state, lane).is_ok()
            || qualified.lane_build_dir(&state, lane).is_ok()
        {
            return Err(fail("lane binding accepted an invalid lane"));
        }
    }
    if qualified
        .lane_target_dir(Path::new("owned/state"), "build")
        .is_ok()
    {
        return Err(fail("lane binding accepted a relative state root"));
    }
    Ok(())
}
