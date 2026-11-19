// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::GenerateError;
use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};
use velnor_release::{FamilyError, ReleaseError};
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, Root, TOOLS, jobs, release_view, steps};
use crate::release_unguarded::expected_release;
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const FAMILY: &str = "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\n";

fn release_plz(root: &Root, release: bool) -> String {
    release_plz_tagged(root, release, "{{ package }}-v{{ version }}")
}

fn release_plz_tagged(root: &Root, release: bool, tag: &str) -> String {
    let listed = if release { "" } else { "release = false\n" };
    format!(
        "[workspace]\ngit_tag_name = \"{tag}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npr_branch_prefix = \"{}\"\ngit_only = true\n\n[[package]]\nname = \"{}\"\n{listed}",
        root.prefix, root.package
    )
}

fn render_family(
    config: &str,
    release: bool,
) -> Result<Result<Node, GenerateError>, Box<dyn Error>> {
    render_family_tagged(config, release, "{{ package }}-v{{ version }}")
}

fn render_family_tagged(
    config: &str,
    release: bool,
    app_tag: &str,
) -> Result<Result<Node, GenerateError>, Box<dyn Error>> {
    let mut view = release_view(&[APP, TOOLS], Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", "0.1.83").into_bytes());
    view.files.insert(
        RepoPath::new("release-plz.toml")?,
        release_plz_tagged(&APP, release, app_tag).into_bytes(),
    );
    view.files.insert(
        RepoPath::new("tools/release-plz.toml")?,
        release_plz(&TOOLS, release).into_bytes(),
    );
    match expected_release(&view) {
        Ok(mut files) => {
            let text = files
                .remove(&RELEASE_WORKFLOW_PATH)
                .ok_or("no release.yml")?;
            Ok(Ok(parse(&text)?))
        }
        Err(error) => Ok(Err(error)),
    }
}

fn family() -> Result<Node, Box<dyn Error>> {
    render_family(FAMILY, false)?.map_err(Into::into)
}

#[test]
fn family_tag_gives_every_binary_one_tag_and_one_publish() -> TestResult {
    let tree = family()?;
    let ids: BTreeSet<&str> = jobs(&tree)?.into_iter().map(|(id, _)| id).collect();
    let want = BTreeSet::from([
        "gate",
        "release-pr",
        "tag",
        "family",
        "family-versions",
        "build-app-linux-x64",
        "build-app-linux-arm64",
        "build-app-macos-arm64",
        "build-tools-linux-x64",
        "build-tools-macos-arm64",
        "sums-family",
        "publish-family",
    ]);
    assert_eq!(ids, want);
    let tag = job(&tree, "tag")?;
    assert!(
        tag.get("outputs")
            .is_none_or(|outputs| outputs.keys().is_empty())
    );
    let publish = job(&tree, "publish-family")?;
    let command = runs(publish).last().copied().ok_or("no publish")?;
    assert!(command.contains("--tag=\"$TAG\""));
    for asset in [
        "app-x86_64-unknown-linux-musl.tar.gz",
        "app-aarch64-unknown-linux-musl.tar.gz",
        "app-aarch64-apple-darwin.tar.gz",
        "tools-x86_64-unknown-linux-musl.tar.gz",
        "tools-aarch64-apple-darwin.tar.gz",
    ] {
        assert!(command.contains(asset), "{asset}");
    }
    Ok(())
}

#[test]
fn family_builds_check_out_the_family_tag_and_refuse_another_commit() -> TestResult {
    let tree = family()?;
    for (id, build) in jobs(&tree)?
        .into_iter()
        .filter(|(id, _)| id.starts_with("build-"))
    {
        let needs = build.get("needs").ok_or("no needs")?.texts();
        assert!(needs.contains(&"family"), "{id}");
        let all = steps(build);
        assert_eq!(
            all[0].at(&["with", "ref"]).and_then(Node::text),
            Some("refs/tags/${{ needs.family.outputs.tag }}"),
            "{id}"
        );
        let check = all[1].get("run").and_then(Node::text).ok_or("no check")?;
        assert!(check.contains("\"$CHECKED_OUT\" = \"$VERIFIED\""), "{id}");
        assert!(check.contains("\"$FAMILY\" = \"$VERIFIED\""), "{id}");
        let condition = build.get("if").and_then(Node::text).ok_or("no if")?;
        assert!(
            condition.contains("needs.family.outputs.build == 'true'"),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn family_job_binds_the_tag_to_the_verified_commit_with_the_helper() -> TestResult {
    let tree = family()?;
    let family = job(&tree, "family")?;
    let command = runs(family)
        .into_iter()
        .find(|run| run.starts_with("velnor-ci release family-tag"))
        .ok_or("no tagging step")?;
    for part in [
        "--verified-commit=\"$COMMIT\"",
        "--tag-pattern='tools-v{{ version }}'",
        "--package=app",
        "--package=tools",
        "family-0.json",
        "family-1.json",
    ] {
        assert!(command.contains(part), "{part}");
    }
    let outputs = family.get("outputs").ok_or("no outputs")?.keys();
    assert_eq!(outputs, ["tag", "commit", "build"]);
    Ok(())
}

#[test]
fn family_tag_needs_a_helper_that_knows_the_command() -> TestResult {
    let mut view = release_view(&[APP, TOOLS], Some(FAMILY))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", "0.1.82").into_bytes());
    for (root, path) in [
        (&APP, "release-plz.toml"),
        (&TOOLS, "tools/release-plz.toml"),
    ] {
        view.files
            .insert(RepoPath::new(path)?, release_plz(root, false).into_bytes());
    }
    assert!(matches!(
        expected_release(&view),
        Err(GenerateError::FamilyTagHelperTooOld(_))
    ));
    Ok(())
}

#[test]
fn family_tag_config_is_refused_when_it_cannot_name_one_tag_for_released_binaries() -> TestResult {
    let named = render_family(
        &FAMILY.replace("tools-v{{ version }}", "{{ package }}-v{{ version }}"),
        false,
    )?;
    assert!(matches!(
        named,
        Err(GenerateError::Release(ReleaseError::Family(
            FamilyError::TagWithPackage { .. }
        )))
    ));
    let released = render_family(FAMILY, true)?;
    assert!(matches!(
        released,
        Err(GenerateError::Release(ReleaseError::Family(
            FamilyError::BinaryReleasedByReleasePlz { .. }
        )))
    ));
    let empty = render_family(
        "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\n",
        false,
    )?;
    assert!(matches!(
        empty,
        Err(GenerateError::Release(ReleaseError::Family(
            FamilyError::WithoutBinaries
        )))
    ));
    Ok(())
}

#[test]
fn the_group_prefixed_family_tag_is_still_refused_when_a_release_plz_root_uses_it() -> TestResult {
    let clash = render_family_tagged(FAMILY, false, "tools-v{{ version }}")?;
    assert!(
        matches!(
            clash,
            Err(GenerateError::Release(
                ReleaseError::TagsMayCollide { .. } | ReleaseError::DuplicateTagPattern { .. }
            ))
        ),
        "a family tag equal to a release-plz tag must be refused"
    );
    let apart = render_family_tagged(FAMILY, false, "app-v{{ version }}")?;
    assert!(apart.is_ok(), "a distinct release-plz tag must pass");
    Ok(())
}
