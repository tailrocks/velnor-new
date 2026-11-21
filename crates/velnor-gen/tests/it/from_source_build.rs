// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::facts::TestResult;
use crate::generated_check_source::{
    FIRST, FROM_SOURCE, check_step, job, names, packages, rendered,
};
use crate::lint_lane::LINT;
use crate::pipeline::workflow;

fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!(
        "velnor-gen-from-source-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("bin"))?;
    fs::create_dir_all(dir.join("repo/tools/gen"))?;
    Ok(dir)
}

fn fake_tool(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn run_names(job: &Value, text: &str) -> Option<usize> {
    job["steps"]
        .as_array()?
        .iter()
        .position(|step| step["run"].as_str().is_some_and(|run| run.contains(text)))
}

#[test]
fn the_source_build_runs_cargo_in_the_generator_directory_for_the_host_target() -> TestResult {
    let dir = scratch("host-target")?;
    let repo = dir.join("repo");
    let work = repo.join("tools/gen");
    let out = dir.join("cargo.txt");
    fake_tool(
        &dir.join("bin/cargo"),
        &format!("pwd > '{0}'; printf '%s\\n' \"$@\" >> '{0}'", out.display()),
    )?;
    let roots = [
        ("Cargo.toml", "velnor-ci-app"),
        ("tools/gen/Cargo.toml", "velnor-gen"),
    ];
    let job = job(&workflow(&packages(
        &format!("{LINT}\n{FROM_SOURCE}"),
        FIRST,
        &roots,
    )?)?)?;
    let step = check_step(&job);
    let status = Command::new("bash")
        .arg("-c")
        .arg(step["run"].as_str().ok_or("no run")?)
        .current_dir(&work)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("bin").display()),
        )
        .status()?;
    assert!(status.success(), "{job}");
    let seen = fs::read_to_string(&out)?;
    let lines: Vec<&str> = seen.lines().collect();
    assert_eq!(
        fs::canonicalize(lines[0])?,
        fs::canonicalize(&work)?,
        "{seen}"
    );
    let at = lines
        .iter()
        .position(|arg| *arg == "--target")
        .ok_or_else(|| format!("no --target in {lines:?}"))?;
    assert_eq!(lines.get(at + 1), Some(&"host-tuple"), "{lines:?}");
    let root = lines
        .iter()
        .position(|arg| *arg == "--root")
        .and_then(|at| lines.get(at + 1))
        .ok_or("no --root")?;
    assert_eq!(
        fs::canonicalize(work.join(root))?,
        fs::canonicalize(&repo)?,
        "{lines:?}"
    );
    fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn from_source_keeps_build_script_outputs_under_target_in_the_private_store() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let store = names(&job)
        .iter()
        .position(|name| name == "Private mbx store")
        .ok_or("no private store")?;
    let outs = run_names(&job, "MBX_SHARE_OUT_DIR=0").ok_or("no out-dirs step")?;
    let tools = names(&job)
        .iter()
        .position(|name| name == "Install tools")
        .ok_or("no tools step")?;
    assert!(store < outs && outs < tools, "{job}");
    Ok(())
}

#[test]
fn the_source_build_asks_cargo_for_the_host_target_instead_of_computing_a_triple() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let run = check_step(&job)["run"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(run.contains(" --target host-tuple "), "{run}");
    assert!(!run.contains("rustc"), "{run}");
    let text = job.to_string();
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
    ] {
        assert!(!text.contains(triple), "{triple}: {job}");
    }
    Ok(())
}

#[test]
fn from_source_builds_the_generator_from_the_workspace_that_holds_it() -> TestResult {
    let roots = [
        ("Cargo.toml", "velnor-ci-app"),
        ("tools/gen/Cargo.toml", "velnor-gen"),
    ];
    let repo = packages(&format!("{LINT}\n{FROM_SOURCE}"), FIRST, &roots)?;
    let job = job(&workflow(&repo)?)?;
    let step = check_step(&job);
    assert_eq!(step["working-directory"], "tools/gen", "{job}");
    assert!(
        step["run"]
            .as_str()
            .is_some_and(|run| run.contains("-- --root ../.. check --verify-release")),
        "{job}"
    );
    Ok(())
}

#[test]
fn from_source_runs_cargo_in_the_generator_directory_for_the_named_binary() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let step = check_step(&job);
    assert!(step["working-directory"].is_null(), "{job}");
    let run = step["run"].as_str().unwrap_or_default();
    assert!(run.contains("--bin velnor-gen"), "{run}");
    assert!(run.contains("-- --root . check"), "{run}");
    Ok(())
}
