// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::{SHELL, job, names, step, steps, view_with};
use crate::yaml_tree::document;

const RESTORING: &str = "[[test_groups]]
name = 'shell'
filter = 'binary(it) and test(~cargo_)'
needs = ['cargo']
restore_cache = true
";

const CACHES: [&str; 2] = ["Restore the sources cache", "Restore the target cache"];

fn rendered_on(config: &str, helper: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(document(&workflow(&view_with(
        &["Cargo.toml"],
        config,
        helper,
    )?)?)?)
}

fn position(job: &Value, name: &str) -> Result<usize, String> {
    names(job)
        .iter()
        .position(|candidate| *candidate == name)
        .ok_or_else(|| format!("no step {name} in {:?}", names(job)))
}

#[test]
fn a_restoring_group_restores_the_build_jobs_caches_under_the_same_keys() -> TestResult {
    let workflow = rendered_on(RESTORING, "0.1.108")?;
    let group = job(&workflow, "rust-test-shell")?;
    let build = job(&workflow, "rust-test")?;
    for name in CACHES {
        let restored = step(group, name)?;
        let original = step(build, name)?;
        assert_eq!(restored["uses"], original["uses"], "{name}");
        assert_eq!(restored["with"]["key"], original["with"]["key"], "{name}");
        assert_eq!(
            restored["with"]["restore-keys"], original["with"]["restore-keys"],
            "{name}"
        );
        assert_eq!(restored["with"]["path"], original["with"]["path"], "{name}");
        let key = restored["with"]["key"].as_str().unwrap_or_default();
        assert!(!key.is_empty(), "{name}");
    }
    let key = step(group, "Restore the target cache")?["with"]["key"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(key.contains("rust-test."), "{key}");
    Ok(())
}

#[test]
fn a_restoring_group_never_saves_and_restores_after_checkout_prepare_and_before_the_tests()
-> TestResult {
    let workflow = rendered_on(RESTORING, "0.1.108")?;
    let group = job(&workflow, "rust-test-shell")?;
    for step in steps(group) {
        let uses = step["uses"].as_str().unwrap_or_default();
        let name = step["name"].as_str().unwrap_or_default();
        assert!(!uses.contains("cache/save"), "{name}");
        assert!(!name.starts_with("Save"), "{name}");
        assert!(!name.contains("Decide"), "{name}");
    }
    assert!(steps(group).iter().any(|step| {
        step["uses"]
            .as_str()
            .unwrap_or_default()
            .contains("cache/restore")
    }));
    let checkout = steps(group)
        .iter()
        .position(|step| {
            step["uses"]
                .as_str()
                .unwrap_or_default()
                .contains("checkout")
        })
        .ok_or("no checkout")?;
    let prepare = steps(group)
        .iter()
        .position(|step| step["id"] == "prepare")
        .ok_or("no prepare")?;
    let download = steps(group)
        .iter()
        .position(|step| {
            step["name"]
                .as_str()
                .unwrap_or_default()
                .starts_with("Download")
        })
        .ok_or("no download")?;
    let test = position(group, "Test")?;
    for name in CACHES {
        let at = position(group, name)?;
        assert!(
            checkout < at && prepare < at && at < download && at < test,
            "{name}"
        );
    }
    assert_eq!(
        group["steps"][0]["uses"]
            .as_str()
            .unwrap_or_default()
            .split('@')
            .next(),
        workflow["jobs"]["rust-test"]["steps"][0]["uses"]
            .as_str()
            .unwrap_or_default()
            .split('@')
            .next()
    );
    let build = job(&workflow, "rust-test")?;
    assert!(
        steps(build).iter().any(|step| step["uses"]
            .as_str()
            .unwrap_or_default()
            .contains("cache/save")),
        "the build job still saves"
    );
    Ok(())
}

#[test]
fn a_group_without_the_key_or_with_it_false_renders_no_cache_restore() -> TestResult {
    let absent = rendered_on(SHELL, "0.1.108")?;
    let off = rendered_on(&format!("{SHELL}restore_cache = false\n"), "0.1.108")?;
    let group = job(&absent, "rust-test-shell")?;
    for name in CACHES {
        assert!(step(group, name).is_err(), "{name}");
    }
    assert_eq!(absent, off);
    let on = rendered_on(RESTORING, "0.1.108")?;
    assert_eq!(
        job(&absent, "rust-test-default")?,
        job(&on, "rust-test-default")?
    );
    assert_eq!(job(&absent, "rust-test")?, job(&on, "rust-test")?);
    Ok(())
}

#[test]
fn restore_cache_needs_the_cargo_need_and_a_helper_that_reads_the_key() -> TestResult {
    let without_cargo = RESTORING.replace("needs = ['cargo']\n", "");
    let error = workflow(&view_with(&["Cargo.toml"], &without_cargo, "0.1.108")?)
        .err()
        .ok_or("accepted without the cargo need")?
        .to_string();
    assert!(
        error.contains("restore_cache") && error.contains("cargo"),
        "{error}"
    );
    for config in [
        RESTORING.to_owned(),
        format!("{SHELL}restore_cache = false\n"),
    ] {
        let error = workflow(&view_with(&["Cargo.toml"], &config, "0.1.107")?)
            .err()
            .ok_or("accepted on a helper that does not read the key")?
            .to_string();
        assert!(
            error.contains("restore_cache") && error.contains("0.1.108"),
            "{error}"
        );
    }
    workflow(&view_with(&["Cargo.toml"], SHELL, "0.1.90")?)?;
    Ok(())
}
