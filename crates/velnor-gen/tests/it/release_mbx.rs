// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};

use crate::release_fixture::{APP, jobs, release_view, steps, uses};
use crate::release_unguarded::expected_release;
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const MISE_ACTION: &str = "jdx/mise-action@";
const MBX_MISE: &str = "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n";
const UNDECLARED_MISE: &str = "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n";

fn release_yml(mise: Option<&str>, config: Option<&[u8]>) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    if let Some(text) = mise {
        view.files
            .insert(RepoPath::new("mise.toml")?, text.as_bytes().to_vec());
    }
    if let Some(bytes) = config {
        view.files
            .insert(RepoPath::new(".velnor/rust-ci.toml")?, bytes.to_vec());
        // `[cache] mbx` needs the helper that reads it.
        crate::pipeline::pin_helper(&mut view, "0.1.108")?;
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
    let tree = release_yml(Some(MBX_MISE), None)?;
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
    let tree = release_yml(Some(UNDECLARED_MISE), Some(b"[cache]\nmbx = false\n"))?;
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
