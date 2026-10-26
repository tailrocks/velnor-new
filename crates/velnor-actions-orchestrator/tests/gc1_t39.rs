// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_contract::config::{CheckExecutor, CheckPlatform, CheckRunner, MiseCheck};
use velnor_actions_contract::graph::{CachePolicy, ResourceClass, ResourceDemand};
use velnor_actions_contract::propose::{IdentityInputs, ProposedTask};
use velnor_actions_mise::checks::CheckCapabilityProof;
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, MISE_VERSION, QualifiedCheck};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn discovered() -> DiscoveredCheck {
    let check = MiseCheck {
        id: "gc1-t39-check".to_owned(),
        task: "lint".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: Vec::new(),
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    let proposal = ProposedTask {
        task_id: "internal/lint/dev".to_owned(),
        stack_id: "mise".to_owned(),
        component_id: "lint".to_owned(),
        task_kind: "check".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Lightweight,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: "internal/lint/dev".to_owned(),
            unit_key: "lint".to_owned(),
            unit_path: "mise.toml".to_owned(),
            project_root: ".".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo".to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("echo")],
        display_name: "lint".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: true,
        runner_profile: "host".to_owned(),
    };
    let task_config =
        "[tasks.lint]\nrun = \"echo ok\"\nsources = [\"a\"]\noutputs = [\"b\"]\n".to_owned();
    DiscoveredCheck {
        check,
        proposal,
        config_inputs: Vec::new(),
        task_config,
        tool_specs: Vec::new(),
        qualified_tools: Vec::new(),
        qualification_digest: "gc1-t39".to_owned(),
        config_source: String::new(),
    }
}

fn qualified_at(home: &Path, cwd: &Path) -> Result<QualifiedCheck, Box<dyn Error>> {
    let found = discovered();
    Ok(QualifiedCheck::new(
        home.to_path_buf(),
        cwd.to_path_buf(),
        found,
        CheckCapabilityProof { container: None },
        &[],
    )?)
}

fn restored_roots(temp: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    (
        temp.join("owned/state"),
        temp.join("owned/cargo"),
        temp.join("owned/rustup"),
        temp.join("owned/tools"),
    )
}

fn stage_command(qualified: &QualifiedCheck, home: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(home.join("bin"))?;
    let mise = home.join("bin/mise");
    fs::write(&mise, format!("#!/bin/sh\necho \"{MISE_VERSION} fake\"\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mise, fs::Permissions::from_mode(0o755))?;
    }
    fs::write(home.join("tasks.toml"), qualified.bound_projection()?)?;
    Ok(())
}

fn overlay_map(qualified: &QualifiedCheck) -> BTreeMap<OsString, OsString> {
    qualified.restored_overlay().iter().cloned().collect()
}

fn check_restored_child_consumes_verified_state() -> Result<(), Box<dyn Error>> {
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

fn check_unprojected_child_keeps_private_homes() -> Result<(), Box<dyn Error>> {
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

fn check_projection_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
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

#[test]
fn gc1_t39_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_restored_child_consumes_verified_state()?;
    check_unprojected_child_keeps_private_homes()?;
    Ok(())
}

#[test]
fn gc1_t39_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_projection_rejects_invalid_state()?;
    Ok(())
}
