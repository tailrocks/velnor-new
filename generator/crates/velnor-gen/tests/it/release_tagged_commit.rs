// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, Root, TOOLS, release_view, steps};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const CHECK: &str = "test \"$CHECKED_OUT\" = \"$COMMIT\"";
const TWO_BINARIES: &str = "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n";

fn render_with(helper: &str, roots: &[Root], config: &str) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(roots, Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", helper).into_bytes());
    let text = expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    Ok(parse(&text)?)
}

fn binaries(helper: &str) -> Result<Node, Box<dyn Error>> {
    render_with(helper, &[APP, TOOLS], TWO_BINARIES)
}

fn the_check(build: &Node) -> Result<&Node, Box<dyn Error>> {
    steps(build)
        .iter()
        .find(|step| step.get("run").and_then(Node::text) == Some(CHECK))
        .ok_or_else(|| "no commit check".into())
}

#[test]
fn the_tag_job_records_the_commit_each_tag_is_on_and_requires_it_in_the_verified_history()
-> TestResult {
    let tree = binaries("0.1.81")?;
    let tag = job(&tree, "tag")?;
    let record = steps(tag)
        .iter()
        .find(|step| step.get("id").and_then(Node::text) == Some("record"))
        .ok_or("no record step")?;
    assert_eq!(
        record.get("run").and_then(Node::text),
        Some(
            "velnor-ci release record --repo \"$REPOSITORY\" --verified-commit \"$COMMIT\" --app velnor-release --config release-plz.toml --config tools/release-plz.toml \"$RUNNER_TEMP/release-0.json\" \"$RUNNER_TEMP/release-1.json\""
        )
    );
    for (key, value) in [
        ("GITHUB_TOKEN", "${{ github.token }}"),
        ("REPOSITORY", "${{ github.repository }}"),
        ("COMMIT", "${{ github.event.workflow_run.head_sha }}"),
    ] {
        assert_eq!(
            record.at(&["env", key]).and_then(Node::text),
            Some(value),
            "{key}"
        );
    }
    assert_eq!(record.get("env").map(Node::keys).map(|k| k.len()), Some(4));
    assert_eq!(
        record.get("if").and_then(Node::text),
        Some("${{ steps.newest.outputs.newest == 'true' }}")
    );
    assert_eq!(
        tag.get("outputs").map(Node::keys),
        Some(vec!["app", "tools", "app_commit", "tools_commit"])
    );
    assert_eq!(
        tag.at(&["outputs", "app_commit"]).and_then(Node::text),
        Some("${{ steps.record.outputs.app_commit }}")
    );
    Ok(())
}

#[test]
fn a_merge_commit_release_builds_the_commit_the_tag_job_resolved_not_the_merge_commit() -> TestResult
{
    let tree = binaries("0.1.81")?;
    let mut checked = 0;
    for (id, package) in [
        ("build-app-linux-x64", "app"),
        ("build-app-linux-arm64", "app"),
        ("build-app-macos-arm64", "app"),
        ("build-tools-linux-x64", "tools"),
    ] {
        let check = the_check(job(&tree, id)?)?;
        assert_eq!(
            check.at(&["env", "COMMIT"]).and_then(Node::text),
            Some(format!("${{{{ needs.tag.outputs.{package}_commit }}}}").as_str()),
            "{id}"
        );
        assert_eq!(
            check.at(&["env", "CHECKED_OUT"]).and_then(Node::text),
            Some("${{ steps.source.outputs.commit }}"),
            "{id}"
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
    Ok(())
}

#[test]
fn a_helper_without_the_commit_flags_keeps_the_head_sha_check_which_a_merge_commit_fails()
-> TestResult {
    let tree = binaries("0.1.80")?;
    let tag = job(&tree, "tag")?;
    assert_eq!(
        runs(tag).last().copied(),
        Some(
            "velnor-ci release record \"$RUNNER_TEMP/release-0.json\" \"$RUNNER_TEMP/release-1.json\""
        )
    );
    assert_eq!(
        tag.get("outputs").map(Node::keys),
        Some(vec!["app", "tools"])
    );
    let check = the_check(job(&tree, "build-app-linux-x64")?)?;
    assert_eq!(
        check.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ github.event.workflow_run.head_sha }}")
    );
    Ok(())
}
