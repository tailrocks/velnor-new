// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{SPACED, render, steps, two_binaries};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

fn job<'a>(tree: &'a Node, id: &str) -> Result<&'a Node, Box<dyn Error>> {
    Ok(tree
        .at(&["jobs", id])
        .ok_or_else(|| format!("no job {id}"))?)
}

fn runs(job: &Node) -> Vec<&str> {
    steps(job)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .collect()
}

#[test]
fn release_yml_runs_cargo_from_the_release_root_with_a_fixed_target_directory_and_the_musl_compiler()
-> TestResult {
    let (_, tree) = two_binaries()?;
    let build = job(&tree, "build-tools-linux-x64")?;
    let cargo = steps(build)
        .iter()
        .find(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.starts_with("cargo "))
        })
        .ok_or("no cargo step")?;
    assert_eq!(
        cargo.get("working-directory").and_then(Node::text),
        Some("tools")
    );
    assert_eq!(
        cargo.at(&["env", "TARGET_CC"]).and_then(Node::text),
        Some("musl-gcc")
    );
    let mac = job(&tree, "build-tools-macos-arm64")?;
    assert!(!runs(mac).iter().any(|run| run.contains("apt-get")),);
    let (_, root) = two_binaries()?;
    let at_root = steps(job(&root, "build-app-linux-x64")?)
        .iter()
        .find(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|r| r.starts_with("cargo "))
        })
        .ok_or("no cargo step")?;
    assert!(at_root.get("working-directory").is_none());
    Ok(())
}

#[test]
fn release_yml_quotes_a_root_with_a_space_in_the_build_commands() -> TestResult {
    let (_, tree) = render(
        &[SPACED],
        Some(
            "roots = [\"my tools\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
        ),
    )?;
    assert_eq!(
        runs(job(&tree, "build-tools-linux-x64")?),
        [
            "sudo apt-get install --yes musl-tools",
            "rustup target add x86_64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path Cargo.toml --package tools --bin tools --target x86_64-unknown-linux-musl --target-dir target",
            "tar -czf tools-linux-x64.tar.gz -C 'my tools/target/x86_64-unknown-linux-musl/release' tools",
        ]
    );
    Ok(())
}
