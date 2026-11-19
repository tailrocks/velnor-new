// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::{FamilyError, ReleaseError};

use crate::release_family_fixture::{HELPER, generate, refused_releasing, tools_like};
use crate::release_fixture::{jobs, steps};
use crate::release_yml_binaries::{job, permissions, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const TAG_PUSH: &str = "family_trigger = \"tag_push\"\ncommit_file = true\n";

#[test]
fn a_tag_push_family_starts_on_the_family_tag_glob_and_has_no_release_plz_jobs() -> TestResult {
    let (text, tree) = generate(HELPER, &tools_like(TAG_PUSH))?;
    assert_eq!(
        tree.at(&["on", "push", "tags"]).map(Node::texts),
        Some(vec!["tools-v*"])
    );
    assert_eq!(tree.get("on").map(Node::keys), Some(vec!["push"]));
    assert!(!text.contains("workflow_run"));
    let ids: BTreeSet<&str> = jobs(&tree)?.into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "gate",
            "family-versions",
            "family",
            "build-app-linux-x64",
            "build-app-linux-arm64",
            "build-tools-linux-x64",
            "build-tools-linux-arm64",
            "sums-family",
            "publish-family",
        ])
    );
    Ok(())
}

#[test]
fn the_gate_reads_the_pushed_commit_and_requires_it_on_the_default_branch() -> TestResult {
    let (_, tree) = generate(HELPER, &tools_like(TAG_PUSH))?;
    let gate = job(&tree, "gate")?;
    let commands = runs(gate);
    assert!(
        commands
            .iter()
            .any(|run| run.contains("compare/$DEFAULT_BRANCH...$COMMIT"))
    );
    let check = steps(gate)
        .iter()
        .find(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.starts_with("velnor-ci release-gate"))
        })
        .ok_or("no release-gate")?;
    assert_eq!(
        check.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ github.sha }}")
    );
    assert_eq!(
        permissions(gate)
            .iter()
            .filter(|(_, access)| access != "read")
            .count(),
        0
    );
    Ok(())
}

#[test]
fn the_family_job_refuses_a_pushed_tag_that_is_not_the_family_tag_of_the_versions() -> TestResult {
    let (_, tree) = generate(HELPER, &tools_like(TAG_PUSH))?;
    let command = runs(job(&tree, "family")?)
        .into_iter()
        .find(|run| run.starts_with("velnor-ci release family-tag"))
        .ok_or("no family-tag")?;
    assert!(command.contains("--pushed-tag=\"$PUSHED_TAG\""));
    assert!(command.contains("--verified-commit=\"$COMMIT\""));
    let family = job(&tree, "family")?;
    let step = steps(family)
        .iter()
        .find(|step| step.get("run").and_then(Node::text) == Some(command))
        .ok_or("no step")?;
    assert_eq!(
        step.at(&["env", "PUSHED_TAG"]).and_then(Node::text),
        Some("${{ github.ref_name }}")
    );
    assert_eq!(
        step.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ github.sha }}")
    );
    Ok(())
}

#[test]
fn tag_push_needs_a_helper_that_checks_the_pushed_tag() -> TestResult {
    match generate("0.1.97", &tools_like(TAG_PUSH)) {
        Err(error) => assert!(matches!(
            error.downcast::<GenerateError>().map(|error| *error),
            Ok(GenerateError::TagPushHelperTooOld(_))
        )),
        Ok(_) => return Err("an old helper was accepted".into()),
    }
    assert!(generate("0.1.97", &tools_like("commit_file = true\n")).is_ok());
    Ok(())
}

#[test]
fn tag_push_is_refused_when_release_plz_still_releases_a_package() -> TestResult {
    let config = "family_tag = \"family-v{{ version }}\"\nfamily_trigger = \"tag_push\"\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n";
    let error = refused_releasing(config, true)?;
    assert!(
        matches!(
            error,
            GenerateError::Release(ReleaseError::Family(
                FamilyError::TagPushWithReleasedPackage { .. }
            ))
        ),
        "{error}"
    );
    Ok(())
}
