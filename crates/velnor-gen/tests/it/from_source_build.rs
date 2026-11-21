// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::facts::TestResult;
use crate::generated_check_source::{FIRST, FROM_SOURCE, check_script, job, names, rendered};
use crate::lint_lane::LINT;

const HOST: &str = "x86_64-unknown-linux-gnu";

fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!(
        "velnor-gen-from-source-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("bin"))?;
    fs::create_dir_all(dir.join("repo"))?;
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
fn the_source_build_passes_the_runner_host_triple_to_cargo() -> TestResult {
    let dir = scratch("host-triple")?;
    let bin = dir.join("bin");
    let repo = dir.join("repo");
    fs::write(repo.join("Cargo.toml"), "")?;
    let args = dir.join("cargo-args.txt");
    fake_tool(&bin.join("rustc"), &format!("echo 'host: {HOST}'"))?;
    fake_tool(
        &bin.join("cargo"),
        &format!("printf '%s\\n' \"$@\" > '{}'", args.display()),
    )?;
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let status = Command::new("bash")
        .arg("-c")
        .arg(check_script(&job))
        .current_dir(&repo)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("VELNOR_GEN_MANIFEST", "Cargo.toml")
        .status()?;
    assert!(status.success(), "{job}");
    let cargo = fs::read_to_string(&args)?;
    let argv: Vec<&str> = cargo.lines().collect();
    let at = argv
        .iter()
        .position(|arg| *arg == "--target")
        .ok_or_else(|| format!("no --target in {argv:?}"))?;
    assert_eq!(argv.get(at + 1), Some(&HOST), "{argv:?}");
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
