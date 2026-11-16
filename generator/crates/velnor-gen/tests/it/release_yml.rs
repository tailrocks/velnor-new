// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};
use velnor_release::ReleaseError;
use velnor_render::{Owner, owner_of};
use velnor_rust::{LOCK_PATH, wanted_files};

use crate::release_fixture::{
    APP, DEFAULT_BRANCH, GREEN, PUSH, READY, Root, app, conjuncts, dashed, jobs, release_view,
    steps, two_binaries, two_roots, uses,
};
use crate::yaml_tree::Node;

const NEWEST: &str = "github.event.workflow_run.head_sha == github.sha";

type TestResult = Result<(), Box<dyn Error>>;
fn package_of<'a>(id: &str, packages: &[&'a str]) -> Result<&'a str, Box<dyn Error>> {
    let rest = ["build-", "sums-", "publish-"]
        .iter()
        .find_map(|kind| id.strip_prefix(kind))
        .ok_or("not a binary job")?;
    packages
        .iter()
        .copied()
        .filter(|package| {
            rest.strip_prefix(package)
                .is_some_and(|tail| tail.is_empty() || tail.starts_with('-'))
        })
        .max_by_key(|package| package.len())
        .ok_or_else(|| format!("no package for {id}").into())
}

fn checkout_of(all: &[Node]) -> Result<&Node, Box<dyn Error>> {
    all.iter()
        .find(|step| uses(step).is_some_and(|u| u.starts_with("actions/checkout@")))
        .ok_or_else(|| "no checkout".into())
}

#[test]
fn release_yml_triggers_only_after_the_named_workflows_complete_on_main() -> TestResult {
    let (_, tree) = two_roots()?;
    assert_eq!(tree.get("on").ok_or("no on")?.keys(), ["workflow_run"]);
    let run = tree.at(&["on", "workflow_run"]).ok_or("no workflow_run")?;
    assert_eq!(
        run.get("workflows").ok_or("no workflows")?.texts(),
        ["Rust CI", "Lint"]
    );
    assert_eq!(run.get("types").ok_or("no types")?.texts(), ["completed"]);
    assert_eq!(run.get("branches").ok_or("no branches")?.texts(), ["main"]);
    for forbidden in [
        "tags",
        "tags-ignore",
        "release",
        "push",
        "pull_request",
        "pull_request_target",
        "workflow_dispatch",
        "schedule",
    ] {
        assert!(
            !tree.get("on").ok_or("no on")?.has_key_anywhere(forbidden),
            "{forbidden}"
        );
    }
    let (_, default) = app()?;
    assert_eq!(
        default
            .at(&["on", "workflow_run", "workflows"])
            .ok_or("no workflows")?
            .texts(),
        ["Rust CI"]
    );
    Ok(())
}

#[test]
fn release_yml_runs_every_job_after_the_gate_only_when_it_is_ready_on_a_green_push() -> TestResult {
    for (_, tree) in [app()?, two_roots()?, two_binaries()?, dashed()?] {
        let all = jobs(&tree)?;
        assert!(all.len() >= 3, "{} jobs", all.len());
        let packages = tree
            .at(&["jobs", "tag", "outputs"])
            .map(Node::keys)
            .unwrap_or_default();
        let (gate_id, gate) = all[0];
        assert_eq!(gate_id, "gate");
        assert_eq!(
            conjuncts(gate),
            BTreeSet::from([PUSH.to_owned(), GREEN.to_owned(), DEFAULT_BRANCH.to_owned()])
        );
        for (id, job) in &all[1..] {
            assert!(
                job.get("needs")
                    .ok_or("no needs")?
                    .texts()
                    .contains(&"gate"),
                "{id}"
            );
            let mut want = BTreeSet::from([
                READY.to_owned(),
                PUSH.to_owned(),
                GREEN.to_owned(),
                DEFAULT_BRANCH.to_owned(),
            ]);
            if *id == "release-pr" {
                want.insert(NEWEST.to_owned());
            }
            if !matches!(*id, "release-pr" | "tag") {
                let package = package_of(id, &packages)?;
                want.insert(format!("needs.tag.outputs.{package} != ''"));
            }
            assert_eq!(conjuncts(job), want, "{id}");
        }
    }
    Ok(())
}

#[test]
fn release_yml_gate_checks_the_commit_ci_ran_on_and_exports_ready() -> TestResult {
    let (_, tree) = app()?;
    let gate = tree.at(&["jobs", "gate"]).ok_or("no gate")?;
    assert_eq!(
        gate.at(&["outputs", "ready"]).and_then(Node::text),
        Some("${{ steps.gate.outputs.ready }}")
    );
    let check = steps(gate)
        .iter()
        .find(|step| step.get("id").and_then(Node::text) == Some("gate"))
        .ok_or("no gate step")?;
    assert_eq!(
        check.get("run").and_then(Node::text),
        Some("velnor-ci release-gate --repo \"$REPOSITORY\" --sha \"$COMMIT\"")
    );
    assert_eq!(
        check.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ github.event.workflow_run.head_sha }}")
    );
    assert_eq!(
        check.at(&["env", "REPOSITORY"]).and_then(Node::text),
        Some("${{ github.repository }}")
    );
    let tag = tree.at(&["jobs", "tag"]).ok_or("no tag")?;
    assert_eq!(
        checkout_of(steps(tag))?
            .at(&["with", "ref"])
            .and_then(Node::text),
        Some("${{ github.event.workflow_run.head_sha }}")
    );
    Ok(())
}

#[test]
fn release_yml_tag_does_not_wait_for_the_newest_commit_in_any_shape() -> TestResult {
    for (text, tree) in [app()?, two_roots()?, two_binaries()?, dashed()?] {
        let tag = tree.at(&["jobs", "tag"]).ok_or("no tag")?;
        assert!(!conjuncts(tag).contains(NEWEST));
        assert!(conjuncts(tag).contains(READY));
        assert!(text.contains(NEWEST));
    }
    Ok(())
}

#[test]
fn release_yml_release_pr_checks_out_the_verified_commit_and_runs_only_while_it_is_newest()
-> TestResult {
    for (_, tree) in [app()?, two_roots()?] {
        let pr = tree.at(&["jobs", "release-pr"]).ok_or("no release-pr")?;
        assert_eq!(
            checkout_of(steps(pr))?
                .at(&["with", "ref"])
                .and_then(Node::text),
            Some("${{ github.event.workflow_run.head_sha }}")
        );
        assert!(conjuncts(pr).contains(NEWEST));
        let tag = tree.at(&["jobs", "tag"]).ok_or("no tag")?;
        assert!(!conjuncts(tag).contains(NEWEST));
    }
    Ok(())
}

#[test]
fn release_yml_serializes_runs_per_commit_and_never_cancels_a_release() -> TestResult {
    let (_, tree) = app()?;
    let group = tree.at(&["concurrency", "group"]).and_then(Node::text);
    assert_eq!(
        group,
        Some("release-${{ github.event.workflow_run.head_sha }}")
    );
    assert_eq!(
        tree.at(&["concurrency", "cancel-in-progress"])
            .and_then(Node::text),
        Some("false")
    );
    let pr = tree
        .at(&["jobs", "release-pr", "concurrency"])
        .ok_or("no group")?;
    assert_eq!(pr.get("group").and_then(Node::text), Some("release-pr"));
    assert_eq!(
        pr.get("cancel-in-progress").and_then(Node::text),
        Some("false")
    );
    let tag = tree
        .at(&["jobs", "tag", "concurrency"])
        .ok_or("no tag group")?;
    assert_eq!(tag.get("group").and_then(Node::text), Some("release-tag"));
    Ok(())
}

#[test]
fn release_yml_is_written_with_the_generated_header_only_when_release_toml_exists() -> TestResult {
    let without = expected_files(&release_view(&[APP], None)?)?;
    assert!(!without.contains_key(&RELEASE_WORKFLOW_PATH));
    let (text, _) = app()?;
    assert_eq!(owner_of(text.as_bytes()), Owner::VelnorGen);
    assert!(
        text.starts_with(
            "# Generated by velnor-gen. Change release-plz.toml or .velnor/release.toml"
        )
    );
    assert_eq!(text, app()?.0);
    Ok(())
}

#[test]
fn release_yml_is_refused_without_a_pinned_helper_and_with_a_broken_release_plz_config()
-> TestResult {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    view.files.remove(&LOCK_PATH);
    view.tracked_paths.remove(&LOCK_PATH);
    assert_eq!(
        expected_files(&view).err(),
        Some(GenerateError::HelperNotPinned)
    );
    let mut view = release_view(&[], Some("[[binaries]]\npackage = \"app\"\n"))?;
    view.files.remove(&RepoPath::new("release-plz.toml")?);
    assert_eq!(
        expected_files(&view).err(),
        Some(GenerateError::Release(
            ReleaseError::MissingReleasePlzToml {
                root: ".".to_owned()
            }
        ))
    );
    Ok(())
}

#[test]
fn release_yml_refuses_a_root_that_would_add_a_second_command() -> TestResult {
    let root = "app; echo";
    let weird = Root {
        dir: "app; echo",
        ..APP
    };
    let view = release_view(&[weird], Some(&format!("roots = [{root:?}]\n")))?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Rules(_))
    ));
    Ok(())
}

#[test]
fn release_yml_inputs_are_read_from_the_tracked_release_files_of_every_root() -> TestResult {
    let tracked: BTreeSet<RepoPath> = [
        ".velnor/release.toml",
        "release-plz.toml",
        "tools/release-plz.toml",
        "tools/deep/release-plz.toml",
        "docs/release-plz.toml.md",
    ]
    .into_iter()
    .map(RepoPath::new)
    .collect::<Result<_, _>>()?;
    let wanted: BTreeSet<String> = wanted_files(&tracked)
        .into_iter()
        .map(|path| path.to_string())
        .collect();
    assert_eq!(
        wanted,
        BTreeSet::from(
            [
                ".velnor/release.toml",
                "release-plz.toml",
                "tools/release-plz.toml",
                "tools/deep/release-plz.toml"
            ]
            .map(str::to_owned)
        )
    );
    Ok(())
}
