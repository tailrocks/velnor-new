// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, TOOLS, jobs, release_view, steps, uses};
use crate::release_unguarded::expected_release;
use crate::release_yml_family::{FAMILY, release_plz};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const MISE_ACTION: &str = "jdx/mise-action@";
const MBX_MISE: &str = "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\n";

fn release_yml(mise: Option<&str>) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    if let Some(text) = mise {
        view.files
            .insert(RepoPath::new("mise.toml")?, text.as_bytes().to_vec());
    }
    let mut files = expected_release(&view)?;
    let text = files
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("release.yml was not rendered")?;
    Ok(parse(&text)?)
}

fn run(step: &Node) -> &str {
    step.get("run").and_then(Node::text).unwrap_or_default()
}

fn install_args(step: &Node) -> Option<&str> {
    step.get("with")
        .and_then(|with| with.get("install_args"))
        .and_then(Node::text)
}

fn is_mise(step: &Node) -> bool {
    uses(step).is_some_and(|used| used.starts_with(MISE_ACTION))
}

#[test]
fn mbx_build_installs_the_mise_tools_before_cargo() -> TestResult {
    let tree = release_yml(Some(MBX_MISE))?;
    let builds: Vec<(&str, &Node)> = jobs(&tree)?
        .into_iter()
        .filter(|(id, _)| id.starts_with("build-"))
        .collect();
    assert!(!builds.is_empty(), "no build job rendered");
    for (id, job) in builds {
        let all = steps(job);
        let install = all
            .iter()
            .position(is_mise)
            .ok_or_else(|| format!("{id}: no mise install step"))?;
        let build = all
            .iter()
            .position(|step| run(step).starts_with("cargo build"))
            .ok_or_else(|| format!("{id}: no cargo build step"))?;
        assert!(
            install < build,
            "{id}: cargo build runs before mise is set up"
        );
        assert_eq!(
            install_args(&all[install]),
            None,
            "{id}: mise must install every tool of mise.toml, including the mbx wrapper"
        );
        for step in all {
            let text = run(step);
            assert!(
                !text.contains("/.cargo/bin/cargo") && !text.contains("rustup run"),
                "{id}: bypasses the mise wrapper: {text}"
            );
        }
    }
    Ok(())
}

#[test]
fn undeclared_build_keeps_the_rust_only_install() -> TestResult {
    let tree = release_yml(None)?;
    for (id, job) in jobs(&tree)? {
        if !id.starts_with("build-") {
            continue;
        }
        let all = steps(job);
        let install = all
            .iter()
            .find(|step| is_mise(step))
            .ok_or_else(|| format!("{id}: no mise install step"))?;
        assert_eq!(install_args(install), Some("rust"), "{id}");
    }
    Ok(())
}

fn family_yml() -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(&[APP, TOOLS], Some(FAMILY))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", "0.1.85").into_bytes());
    view.files.insert(
        RepoPath::new("release-plz.toml")?,
        release_plz(&APP, false).into_bytes(),
    );
    view.files.insert(
        RepoPath::new("tools/release-plz.toml")?,
        release_plz(&TOOLS, false).into_bytes(),
    );
    view.files
        .insert(RepoPath::new("mise.toml")?, MBX_MISE.as_bytes().to_vec());
    let mut files = expected_release(&view)?;
    let text = files
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("release.yml was not rendered")?;
    Ok(parse(&text)?)
}

/// Release builds always use a fresh private mbx store; never shared, never restored.
fn assert_private_before_cargo(tree: &Node, expect: &[&str]) -> TestResult {
    let mut seen = Vec::new();
    for (id, job) in jobs(tree)? {
        let all = steps(job);
        let Some(first) = all
            .iter()
            .position(|step| is_mise(step) || run(step).starts_with("cargo "))
        else {
            continue;
        };
        seen.push(id);
        let private = all
            .iter()
            .position(|step| step.get("name").and_then(Node::text) == Some("Private mbx store"))
            .ok_or_else(|| format!("{id}: no private mbx store step"))?;
        assert!(private < first, "{id}: the private store comes after cargo or mise");
        let text = run(&all[private]);
        assert_eq!(
            text,
            format!("echo \"MBX_CACHE_DIR=$RUNNER_TEMP/mbx-release-{id}\" >> \"$GITHUB_ENV\""),
            "{id}: not a job-private store"
        );
    }
    for id in expect {
        assert!(seen.contains(id), "{id} was not checked, saw {seen:?}");
    }
    Ok(())
}

#[test]
fn every_cargo_job_makes_a_fresh_private_store_first() -> TestResult {
    assert_private_before_cargo(&release_yml(Some(MBX_MISE))?, &["build-app-linux-x64"])?;
    assert_private_before_cargo(
        &family_yml()?,
        &["family-versions", "build-app-linux-x64", "build-tools-linux-x64"],
    )
}

#[test]
fn no_release_job_shares_restores_or_saves_an_mbx_store() -> TestResult {
    for tree in [release_yml(Some(MBX_MISE))?, family_yml()?] {
        for (id, job) in jobs(&tree)? {
            for step in steps(job) {
                let used = uses(step).unwrap_or_default();
                assert!(
                    !used.starts_with("actions/cache") && !used.contains("mbx"),
                    "{id}: restores or saves a cache: {used}"
                );
                let text = format!("{step:?}");
                for banned in ["MBX_REMOTE", "VELNOR_MBX", "mbx_shared_store", "mbx-store"] {
                    assert!(!text.contains(banned), "{id}: mentions {banned}");
                }
            }
        }
    }
    Ok(())
}

#[test]
fn undeclared_release_sets_no_mbx_store() -> TestResult {
    for (id, job) in jobs(&release_yml(None)?)? {
        for step in steps(job) {
            assert!(!run(step).contains("MBX_CACHE_DIR"), "{id}");
        }
    }
    Ok(())
}
