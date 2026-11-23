// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::generated_check_source::{
    FIRST, FROM_SOURCE, check_step, job, names, packages, rendered,
};
use crate::lint_lane::LINT;
use crate::pipeline::workflow;

fn run_names(job: &Value, text: &str) -> Option<usize> {
    job["steps"]
        .as_array()?
        .iter()
        .position(|step| step["run"].as_str().is_some_and(|run| run.contains(text)))
}

#[test]
fn the_source_build_runs_cargo_in_the_generator_directory_for_the_host_target() -> TestResult {
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
    let work = step["working-directory"]
        .as_str()
        .ok_or_else(|| format!("no working-directory: {job}"))?;
    assert_eq!(work, "tools/gen", "{job}");
    let words: Vec<&str> = step["run"]
        .as_str()
        .ok_or("no run")?
        .split_whitespace()
        .collect();
    let at = words
        .iter()
        .position(|arg| *arg == "--target")
        .ok_or_else(|| format!("no --target in {words:?}"))?;
    assert_eq!(words.get(at + 1), Some(&"host-tuple"), "{words:?}");
    let root = words
        .iter()
        .position(|arg| *arg == "--root")
        .and_then(|at| words.get(at + 1))
        .ok_or("no --root")?;
    let mut resolved: Vec<&str> = work.split('/').collect();
    for part in root.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                resolved.pop();
            }
            other => resolved.push(other),
        }
    }
    assert!(resolved.is_empty(), "{root} from {work}: {resolved:?}");
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
