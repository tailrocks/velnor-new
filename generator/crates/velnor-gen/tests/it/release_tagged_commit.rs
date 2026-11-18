// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, Root, TOOLS, release_view, steps};
use crate::release_unguarded::expected_release;
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::{Node, parse};
use velnor_gen::GenerateError;

type TestResult = Result<(), Box<dyn Error>>;

const CHECK: &str = "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"";
const TWO_BINARIES: &str = "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n";

pub(crate) fn render_with(
    helper: &str,
    roots: &[Root],
    config: &str,
) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(roots, Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", helper).into_bytes());
    let text = expected_release(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    Ok(parse(&text)?)
}

fn binaries(helper: &str) -> Result<Node, Box<dyn Error>> {
    render_with(helper, &[APP, TOOLS], TWO_BINARIES)
}

fn the_check(build: &Node) -> Result<&Node, Box<dyn Error>> {
    find_check(build, CHECK)
}

fn find_check<'a>(build: &'a Node, check: &str) -> Result<&'a Node, Box<dyn Error>> {
    steps(build)
        .iter()
        .find(|step| step.get("run").and_then(Node::text) == Some(check))
        .ok_or_else(|| "no commit check".into())
}

#[test]
fn the_tag_job_records_the_commit_each_tag_is_on_and_requires_it_to_equal_the_verified_commit()
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
            "velnor-ci release record --repo=\"$REPOSITORY\" --verified-commit=\"$COMMIT\" \"$RUNNER_TEMP/release-0.json\" \"$RUNNER_TEMP/release-1.json\""
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
    assert_eq!(record.get("env").map(Node::keys).map(|k| k.len()), Some(3));
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
fn the_tag_job_checks_the_release_source_read_only_before_the_app_token_is_minted() -> TestResult {
    let tree = binaries("0.1.81")?;
    let tag = job(&tree, "tag")?;
    let all = steps(tag);
    let position = |pick: &dyn Fn(&Node) -> bool| all.iter().position(pick);
    let preflight = position(&|step| {
        step.get("run")
            .and_then(Node::text)
            .is_some_and(|run| run.starts_with("velnor-ci release preflight"))
    })
    .ok_or("no preflight step")?;
    let token = position(&|step| step.get("id").and_then(Node::text) == Some("app"))
        .ok_or("no app token step")?;
    let first_release_plz = position(&|step| {
        step.get("run")
            .and_then(Node::text)
            .is_some_and(|run| run.starts_with("\"$RUNNER_TEMP/release-plz\" release"))
    })
    .ok_or("no release-plz step")?;
    assert!(preflight < token && token < first_release_plz);
    let step = &all[preflight];
    assert_eq!(
        step.get("run").and_then(Node::text),
        Some(
            "velnor-ci release preflight --repo=\"$REPOSITORY\" --verified-commit=\"$COMMIT\" --branch-prefix=release-plz-app- --branch-prefix=release-plz-tools-"
        )
    );
    assert_eq!(
        step.get("if").and_then(Node::text),
        Some("${{ steps.newest.outputs.newest == 'true' }}")
    );
    assert_eq!(
        tag.at(&["permissions", "pull-requests"])
            .and_then(Node::text),
        Some("read")
    );
    let older = binaries("0.1.80")?;
    assert!(
        runs(job(&older, "tag")?)
            .iter()
            .all(|run| !run.contains("preflight"))
    );
    assert_eq!(
        job(&older, "tag")?.at(&["permissions", "pull-requests"]),
        None
    );
    Ok(())
}

#[test]
fn a_build_compares_its_checkout_with_the_commit_the_tag_job_recorded() -> TestResult {
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
    let check = find_check(
        job(&tree, "build-app-linux-x64")?,
        "test \"$CHECKED_OUT\" = \"$COMMIT\"",
    )?;
    assert_eq!(
        check.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ github.event.workflow_run.head_sha }}")
    );
    Ok(())
}

#[test]
fn a_build_refuses_an_empty_commit_output_instead_of_comparing_two_empty_strings() -> TestResult {
    let tree = binaries("0.1.81")?;
    let check = the_check(job(&tree, "build-app-linux-x64")?)?;
    let run = check.get("run").and_then(Node::text).ok_or("no run")?;
    let status = std::process::Command::new("/bin/bash")
        .args(["-ec", run])
        .env("COMMIT", "")
        .env("CHECKED_OUT", "")
        .status()?;
    assert!(!status.success());
    let status = std::process::Command::new("/bin/bash")
        .args(["-ec", run])
        .env("COMMIT", "abc")
        .env("CHECKED_OUT", "abc")
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn a_package_named_like_another_packages_commit_output_is_refused_at_generation() -> TestResult {
    const SHADOW: Root = Root {
        dir: "tools",
        package: "app_commit",
        prefix: "release-plz-shadow-",
        publishes: false,
    };
    let mut view = release_view(
        &[APP, SHADOW],
        Some("roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n"),
    )?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", "0.1.81").into_bytes());
    let Err(GenerateError::Rules(violations)) = expected_release(&view) else {
        return Err("rendered".into());
    };
    assert_eq!(violations.len(), 1);
    assert_eq!(
        violations[0].to_string(),
        "the tag job would define the output app_commit twice: it is both a released package and the commit output of another package; rename the package"
    );
    Ok(())
}

#[test]
fn an_assetless_package_named_like_another_packages_commit_output_is_refused_at_generation()
-> TestResult {
    const SHADOW: Root = Root {
        dir: "tools",
        package: "app_commit",
        prefix: "release-plz-shadow-",
        publishes: false,
    };
    let mut view = release_view(&[APP, SHADOW], Some("roots = [\".\", \"tools\"]\n"))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", "0.1.81").into_bytes());
    let Err(GenerateError::Rules(violations)) = expected_release(&view) else {
        return Err("rendered".into());
    };
    assert_eq!(violations.len(), 1);
    assert_eq!(
        violations[0].to_string(),
        "the tag job would define the output app_commit twice: it is both a released package and the commit output of another package; rename the package"
    );
    Ok(())
}

#[test]
fn a_prefix_that_looks_like_an_option_is_attached_to_its_flag() -> TestResult {
    const HELP: Root = Root {
        dir: ".",
        package: "app",
        prefix: "-h",
        publishes: false,
    };
    let tree = render_with("0.1.81", &[HELP], "[[binaries]]\npackage = \"app\"\n")?;
    let run = steps(job(&tree, "tag")?)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .find(|run| run.starts_with("velnor-ci release preflight"))
        .ok_or("no preflight step")?;
    assert!(run.ends_with(" --branch-prefix=-h"), "{run}");
    Ok(())
}
