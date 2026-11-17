// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, TOOLS, release_view, steps};
use crate::release_yml_binaries::job;
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

fn render(helper: &str) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(
        &[APP, TOOLS],
        Some("roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n"),
    )?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", helper).into_bytes());
    let text = expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    Ok(parse(&text)?)
}

fn position(all: &[Node], matches: &dyn Fn(&Node) -> bool) -> Option<usize> {
    all.iter().position(matches)
}

fn run_of(step: &Node) -> &str {
    step.get("run").and_then(Node::text).unwrap_or_default()
}

#[test]
fn the_tag_job_binds_each_live_branch_prefix_before_the_app_token_is_minted() -> TestResult {
    let tree = render("0.1.81")?;
    let all = steps(job(&tree, "tag")?);
    let check = position(all, &|step| {
        run_of(step).starts_with("velnor-ci release check-config")
    })
    .ok_or("no check-config step")?;
    let token = position(all, &|step| {
        step.get("id").and_then(Node::text) == Some("app")
    })
    .ok_or("no app token step")?;
    assert!(check < token);
    let run = run_of(&all[check]);
    for flag in [
        "--branch-prefix release-plz.toml release-plz-app-",
        "--branch-prefix tools/release-plz.toml release-plz-tools-",
    ] {
        assert!(run.contains(flag), "{run}");
    }
    Ok(())
}

#[test]
fn the_release_pr_job_and_an_older_helper_bind_no_branch_prefix() -> TestResult {
    let tree = render("0.1.81")?;
    assert!(
        steps(job(&tree, "release-pr")?)
            .iter()
            .all(|step| !run_of(step).contains("--branch-prefix"))
    );
    let older = render("0.1.80")?;
    assert!(
        steps(job(&older, "tag")?)
            .iter()
            .all(|step| !run_of(step).contains("--branch-prefix"))
    );
    Ok(())
}
