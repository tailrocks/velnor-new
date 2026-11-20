// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use velnor_model::RepoView;

use crate::facts::{TestResult, path};
use crate::lint_lane::{MISE, repo, runs};
use crate::pipeline::workflow;

const JACTIONLINT: &str = "[[lint]]\nkind = 'jactionlint'\n";
const SCALE_SET: &str = "runner = 'ubuntu-26.04-scale-set'\n";

fn with_mise(config: &str, extra: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = repo(config)?;
    view.files
        .insert(path("mise.toml")?, format!("{MISE}{extra}").into_bytes());
    Ok(view)
}

fn tools(text: &str) -> String {
    format!(
        "[wrappers.cargo]\ncommand = \"mbx\"\nenv = {{ MBX_CARGO_SHIM_MODE = \"1\" }}\n\n[tools]\nrust = {{ version = \"1.99.1\", mr_boxington = true }}\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n{text}\n"
    )
}

fn replacing_tools(config: &str, text: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = repo(config)?;
    view.files
        .insert(path("mise.toml")?, tools(text).into_bytes());
    Ok(view)
}

fn refused(view: &RepoView, mention: &str) -> TestResult {
    let error = workflow(view).err().ok_or("accepted")?;
    assert!(error.to_string().contains(mention), "{mention}: {error}");
    Ok(())
}

fn install_args(view: &RepoView, job: &str) -> Result<String, Box<dyn std::error::Error>> {
    let jobs = crate::yaml_tree::document(&workflow(view)?)?["jobs"].clone();
    Ok(jobs[job]["steps"][1]["with"]["install_args"]
        .as_str()
        .ok_or("no install_args")?
        .to_owned())
}

fn lock(entries: &[(&str, &str)]) -> Result<String, std::fmt::Error> {
    let mut text = "[[tools.mr-boxington]]\nversion = \"1.30.0\"\nspecifiers = [\"v1.30.0\"]\n\n[tools.mr-boxington.\"platforms.linux-x64\"]\nurl = \"https://tools.test/mbx\"\n\n".to_owned();
    for (name, version) in entries {
        write!(
            text,
            "[[tools.\"{name}\"]]\nversion = \"{version}\"\n\n[tools.\"{name}\".\"platforms.linux-x64\"]\nurl = \"https://tools.test/{name}\"\n\n"
        )?;
    }
    Ok(text)
}

#[test]
fn a_lint_tool_filtered_out_by_os_on_the_runner_is_refused() -> TestResult {
    let config = format!("{SCALE_SET}{JACTIONLINT}");
    let other_os = replacing_tools(
        &config,
        "jactionlint = { version = \"2.4.0\", os = [\"macos\"] }\n",
    )?;
    refused(&other_os, "filtered out by os")?;
    let this_os = replacing_tools(
        &config,
        "jactionlint = { version = \"2.4.0\", os = [\"linux\"] }\n",
    )?;
    assert_eq!(install_args(&this_os, "lint-jactionlint")?, "jactionlint");
    Ok(())
}

#[test]
fn a_shellcheck_filtered_out_on_the_runner_is_not_installed() -> TestResult {
    let view = replacing_tools(
        &format!("{SCALE_SET}{JACTIONLINT}"),
        "jactionlint = \"2.4.0\"\nshellcheck = { version = \"0.11.0\", os = [\"macos\"] }\n",
    )?;
    assert_eq!(install_args(&view, "lint-jactionlint")?, "jactionlint");
    let jobs = crate::yaml_tree::document(&workflow(&view)?)?["jobs"].clone();
    assert!(runs(&jobs["lint-jactionlint"])[0].ends_with("--shellcheck="));
    Ok(())
}

#[test]
fn a_lint_tool_name_that_matches_two_configured_tools_is_refused_until_it_is_qualified()
-> TestResult {
    let both = "\"aqua:o/t/jactionlint\" = \"2.4.0\"\n\"ubi:o/jactionlint\" = \"2.4.0\"\n";
    refused(
        &replacing_tools(&format!("{SCALE_SET}{JACTIONLINT}"), both)?,
        "name one of them in full",
    )?;
    let task = "[[lint]]\nkind = 'mise-task'\ntask = 'check'\ntools = ['ubi:o/jactionlint']\n";
    let mut view = replacing_tools(&format!("{SCALE_SET}{task}"), both)?;
    view.files.insert(
        path("mise.toml")?,
        format!("{}\n[tasks.check]\nrun = \"true\"\n", tools(both)).into_bytes(),
    );
    assert_eq!(install_args(&view, "lint-check")?, "ubi:o/jactionlint");
    Ok(())
}

#[test]
fn a_mise_task_that_no_task_table_or_task_file_defines_is_refused() -> TestResult {
    let typo = "[[lint]]\nkind = 'mise-task'\ntask = 'chek'\n";
    refused(&with_mise(typo, "")?, "chek")?;
    let known = "[[lint]]\nkind = 'mise-task'\ntask = 'docs:lint'\n";
    refused(&with_mise(known, "")?, "docs:lint")?;
    let mut view = with_mise(known, "")?;
    view.tracked_paths.insert(path("mise-tasks/docs/lint.sh")?);
    workflow(&view)?;
    let toml = "[[lint]]\nkind = 'mise-task'\ntask = 'check'\n";
    workflow(&with_mise(toml, "")?)?;
    Ok(())
}

#[test]
fn a_task_that_uses_a_tool_lint_tools_omits_is_refused_even_through_depends() -> TestResult {
    let pins = "jq = \"1.8.1\"\njactionlint = \"2.4.0\"\n";
    let tasks = "\n[tasks.base]\nrun = \"true\"\ntools = { jq = \"1.8.1\" }\n\n[tasks.check]\nrun = \"true\"\ndepends = [\"base\"]\n";
    let omitted = "[[lint]]\nkind = 'mise-task'\ntask = 'check'\ntools = ['jactionlint']\n";
    let mut view = replacing_tools(omitted, pins)?;
    view.files.insert(
        path("mise.toml")?,
        format!("{}{tasks}", tools(pins)).into_bytes(),
    );
    refused(&view, "jq")?;
    let named = "[[lint]]\nkind = 'mise-task'\ntask = 'check'\ntools = ['jq']\n";
    let mut view = replacing_tools(named, pins)?;
    view.files.insert(
        path("mise.toml")?,
        format!("{}{tasks}", tools(pins)).into_bytes(),
    );
    assert_eq!(install_args(&view, "lint-check")?, "jq");
    Ok(())
}

#[test]
fn jactionlint_with_github_script_steps_needs_a_pinned_node_and_installs_it() -> TestResult {
    let workflow_file = path(".github/workflows/ci.yml")?;
    let script = b"jobs:\n  a:\n    steps:\n      - uses: actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3\n".to_vec();
    let config = format!("{SCALE_SET}{JACTIONLINT}");
    let mut without = replacing_tools(&config, "jactionlint = \"2.4.0\"\n")?;
    without.files.insert(workflow_file.clone(), script.clone());
    refused(&without, "node")?;
    let pins = "jactionlint = \"2.4.0\"\nnode = \"24.11.0\"\n";
    let mut with = replacing_tools(&config, pins)?;
    with.files.insert(workflow_file, script);
    assert_eq!(install_args(&with, "lint-jactionlint")?, "jactionlint node");
    Ok(())
}

#[test]
fn a_repository_without_github_script_steps_needs_no_node() -> TestResult {
    let view = replacing_tools(
        &format!("{SCALE_SET}{JACTIONLINT}"),
        "jactionlint = \"2.4.0\"\n",
    )?;
    assert_eq!(install_args(&view, "lint-jactionlint")?, "jactionlint");
    Ok(())
}

#[test]
fn a_lint_pin_that_mise_lock_holds_at_another_version_is_refused() -> TestResult {
    let config = format!("{SCALE_SET}{JACTIONLINT}");
    let mut view = replacing_tools(&config, "jactionlint = \"2.4.0\"\n")?;
    view.files.insert(
        path("mise.lock")?,
        lock(&[
            ("aqua:nextest-rs/nextest/cargo-nextest", "0.9.151"),
            ("jactionlint", "2.3.0"),
        ])?
        .into_bytes(),
    );
    view.tracked_paths.insert(path("mise.lock")?);
    refused(&view, "no entry for jactionlint at 2.4.0")?;
    view.files.insert(
        path("mise.lock")?,
        lock(&[
            ("aqua:nextest-rs/nextest/cargo-nextest", "0.9.151"),
            ("jactionlint", "2.4.0"),
        ])?
        .into_bytes(),
    );
    workflow(&view)?;
    Ok(())
}

#[test]
fn a_lint_job_runs_on_every_ready_pull_request_even_when_ci_is_label_gated() -> TestResult {
    let mut view = crate::base_save::pinned("0.1.89", &format!("pr_ci = 'label'\n{JACTIONLINT}"))?;
    view.files
        .insert(path("mise.toml")?, MISE.as_bytes().to_vec());
    let jobs = crate::yaml_tree::document(&workflow(&view)?)?["jobs"].clone();
    let condition = jobs["lint-jactionlint"]["if"]
        .as_str()
        .ok_or("no condition")?;
    assert!(
        condition.contains("pull_request.draft != true"),
        "{condition}"
    );
    assert!(!condition.contains("pull_request.labels"), "{condition}");
    Ok(())
}
