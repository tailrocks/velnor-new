// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RepoPath, RepoView};

use crate::facts::{TestResult, lock_pinned_tools};
use crate::pipeline::{pin_helper, view, workflow};
use crate::yaml_tree::document;

const ON: &[u8] = b"[cache]\nmbx_remote = \"github-actions\"\n";
const ACTION: &str = "jdx/mr-boxington-action@";
const PIN_RUN: &str = ". \"$VELNOR_EXEC\" mise-pin mr-boxington";
const PIN_OUTPUT: &str = "${{ steps.mbx-pin.outputs.version }}";

fn repo(helper: &str, mbx: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = view(&["Cargo.toml"], Some(ON))?;
    let mise = format!(
        "[tools]\nrust = {{ version = \"1.99.1\", mr_boxington = true }}\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n{mbx}\n"
    );
    repo.files
        .insert(RepoPath::new("mise.toml")?, mise.into_bytes());
    repo.files.remove(&RepoPath::new("mise.lock")?);
    lock_pinned_tools(&mut repo)?;
    pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn text(repo: &RepoView) -> Result<String, Box<dyn Error>> {
    workflow(repo)
}

#[test]
fn the_action_version_is_the_mise_pin_read_at_run_time_not_a_copied_release() -> TestResult {
    let text = text(&repo("0.1.109", "mr-boxington = \"v1.31.0\"")?)?;
    assert!(
        !text.contains("1.30.0"),
        "the old constant is still rendered"
    );
    let yaml = document(&text)?;
    let mut seen = 0;
    for (id, job) in yaml["jobs"].as_object().ok_or("no jobs")? {
        let steps = job["steps"].as_array().ok_or("no steps")?;
        let Some(mbx) = steps
            .iter()
            .position(|s| s["uses"].as_str().is_some_and(|u| u.starts_with(ACTION)))
        else {
            continue;
        };
        seen += 1;
        assert_eq!(steps[mbx]["with"]["version"], PIN_OUTPUT, "{id}");
        let pin = steps
            .iter()
            .position(|s| s["id"] == "mbx-pin")
            .ok_or_else(|| format!("{id}: no mbx-pin step"))?;
        assert_eq!(steps[pin]["run"], PIN_RUN, "{id}");
        assert!(
            pin < mbx,
            "{id}: the version is read after the action needs it"
        );
    }
    assert_ne!(seen, 0);
    Ok(())
}

#[test]
fn an_older_helper_keeps_the_constant_and_renders_no_pin_step() -> TestResult {
    for helper in ["0.1.105", "0.1.108"] {
        for mbx in ["mr-boxington = \"v1.31.0\"", "mr-boxington = \"v1.30.0\""] {
            let text = text(&repo(helper, mbx)?)?;
            assert!(text.contains("version: '1.30.0'"), "{helper} {mbx}");
            assert!(!text.contains("mise-pin"), "{helper} {mbx}");
        }
    }
    Ok(())
}

#[test]
fn a_missing_mbx_pin_is_a_generation_error_when_the_helper_reads_it() -> TestResult {
    let error = text(&repo("0.1.109", "")?)
        .err()
        .ok_or("generated without a pin")?;
    assert!(error.to_string().contains("mr-boxington"), "{error}");
    Ok(())
}

#[test]
fn a_range_or_latest_mbx_pin_is_a_generation_error() -> TestResult {
    for pin in ["^1.30.0", "latest", "1.30"] {
        let mbx = format!("mr-boxington = \"{pin}\"");
        assert!(text(&repo("0.1.109", &mbx)?).is_err(), "{pin} was accepted");
    }
    Ok(())
}

fn with_second_config(pin: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = repo("0.1.109", "mr-boxington = \"v1.31.0\"")?;
    repo.files.insert(
        RepoPath::new(".mise.toml")?,
        format!("[tools]\nmr-boxington = \"{pin}\"\n").into_bytes(),
    );
    Ok(repo)
}

#[test]
fn two_configs_pinning_different_mbx_releases_are_refused_naming_both() -> TestResult {
    let error = text(&with_second_config("v1.32.0")?)
        .err()
        .ok_or("generated from two different pins")?
        .to_string();
    assert!(error.contains("mise.toml"), "{error}");
    assert!(error.contains(".mise.toml"), "{error}");
    assert!(error.contains("tools.mr-boxington"), "{error}");
    Ok(())
}

#[test]
fn two_configs_pinning_the_same_mbx_release_are_refused_naming_both() -> TestResult {
    let error = text(&with_second_config("v1.31.0")?)
        .err()
        .ok_or("generated from two configs")?
        .to_string();
    assert!(error.contains(".mise.toml"), "{error}");
    Ok(())
}

#[test]
fn without_the_action_the_pin_is_not_read() -> TestResult {
    let mut plain = view(&["Cargo.toml"], None)?;
    pin_helper(&mut plain, "0.1.109")?;
    let text = text(&plain)?;
    assert!(!text.contains("mise-pin"));
    assert!(!text.contains("mr-boxington"));
    Ok(())
}
