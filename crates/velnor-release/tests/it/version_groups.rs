// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A release-plz `version_group` follower turns its own tag and GitHub release
//! off; its group's lead owns both. The groups of this repository's own
//! `release-plz.toml` (#1738) must validate, and the safety of the
//! `git_tag_enable`/`git_release_enable` check must stay for everything else.

use velnor_release::{GroupError, ReleaseError, ReleasePlan};

use crate::fixture::{TestResult, files, refusal, with_metadata};

const HEAD: &str = "[workspace]\nrelease = false\nrelease_always = false\nsemver_check = false\ngit_only = true\ngit_release_draft = true\n";
const OFF: &str = "git_tag_enable = false\ngit_release_enable = false\n";

fn package(name: &str, tag: &str, group: Option<&str>, extra: &str) -> String {
    let group = group.map_or_else(String::new, |g| format!("version_group = \"{g}\"\n"));
    format!(
        "\n[[package]]\nname = \"{name}\"\nrelease = true\ngit_tag_name = \"{tag}\"\n{group}{extra}"
    )
}

const TOOLS: &str = "tools-v{{ version }}";

fn plz(packages: &[String]) -> String {
    format!("{HEAD}{}", packages.concat())
}

fn refused(text: String) -> Option<ReleaseError> {
    let mut files = files();
    files.release_plz_toml.insert(".".to_owned(), text);
    refusal(&files)
}

fn switch_off(error: Option<&ReleaseError>, name: &str) -> bool {
    matches!(error, Some(ReleaseError::ReleaseSwitchOff { package: Some(p), .. }) if p == name)
}

#[test]
fn a_group_follower_with_its_tag_and_release_off_is_accepted() {
    let text = plz(&[
        package("lead", TOOLS, Some("tools"), ""),
        package("follower", TOOLS, Some("tools"), OFF),
    ]);
    let error = refused(text);
    assert!(error.is_none(), "{error:?}");
}

#[test]
fn a_follower_outside_any_group_is_still_refused() {
    let text = plz(&[
        package("lead", TOOLS, Some("tools"), ""),
        package("follower", TOOLS, None, OFF),
    ]);
    assert!(switch_off(refused(text).as_ref(), "follower"));
    let alone = plz(&[package("solo", TOOLS, None, "git_tag_enable = false\n")]);
    assert!(switch_off(refused(alone).as_ref(), "solo"));
}

#[test]
fn a_follower_that_only_turns_one_switch_off_is_still_refused() {
    for extra in ["git_tag_enable = false\n", "git_release_enable = false\n"] {
        let text = plz(&[
            package("lead", TOOLS, Some("tools"), ""),
            package("follower", TOOLS, Some("tools"), extra),
        ]);
        assert!(switch_off(refused(text).as_ref(), "follower"), "{extra}");
    }
}

#[test]
fn a_group_without_a_tagging_lead_is_refused() {
    let text = plz(&[
        package("a", TOOLS, Some("tools"), OFF),
        package("b", TOOLS, Some("tools"), OFF),
    ]);
    let error = refused(text);
    assert!(
        matches!(
            error,
            Some(ReleaseError::Group(GroupError::Leads { count: 0, .. }))
        ),
        "{error:?}"
    );
    let other = plz(&[
        package("lead", TOOLS, Some("other"), ""),
        package("follower", TOOLS, Some("tools"), OFF),
    ]);
    let error = refused(other);
    assert!(
        matches!(
            error,
            Some(ReleaseError::Group(GroupError::Leads { count: 0, .. }))
        ),
        "{error:?}"
    );
}

#[test]
fn a_group_with_two_tagging_leads_and_a_follower_is_refused() {
    let text = plz(&[
        package("a", TOOLS, Some("tools"), ""),
        package("b", TOOLS, Some("tools"), ""),
        package("c", TOOLS, Some("tools"), OFF),
    ]);
    let error = refused(text);
    assert!(
        matches!(
            error,
            Some(ReleaseError::Group(GroupError::Leads { count: 2, .. }))
        ),
        "{error:?}"
    );
}

#[test]
fn a_follower_whose_tag_template_differs_from_the_lead_is_refused() {
    let text = plz(&[
        package("lead", TOOLS, Some("tools"), ""),
        package("follower", "other-v{{ version }}", Some("tools"), OFF),
    ]);
    let error = refused(text);
    assert!(
        matches!(error, Some(ReleaseError::Group(GroupError::Tag { package: ref p, .. })) if p == "follower"),
        "{error:?}"
    );
    let per_package = plz(&[
        package("lead", "{{ package }}-v{{ version }}", Some("tools"), ""),
        package(
            "follower",
            "{{ package }}-v{{ version }}",
            Some("tools"),
            OFF,
        ),
    ]);
    assert!(matches!(
        refused(per_package),
        Some(ReleaseError::Group(GroupError::Tag { .. }))
    ));
}

#[test]
fn groups_in_one_root_still_may_not_share_a_tag_with_another_group_or_package() {
    let text = plz(&[
        package("lead", TOOLS, Some("tools"), ""),
        package("follower", TOOLS, Some("tools"), OFF),
        package("outsider", TOOLS, None, ""),
    ]);
    let error = refused(text);
    assert!(
        matches!(
            error,
            Some(ReleaseError::DuplicateTagPattern { .. } | ReleaseError::TagsMayCollide { .. })
        ),
        "{error:?}"
    );
}

fn real_files(
    release_toml: &str,
) -> Result<velnor_release::ReleaseFiles, Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../release-plz.toml");
    let mut files = files();
    files
        .release_plz_toml
        .insert(".".to_owned(), std::fs::read_to_string(path)?);
    files.release_toml = Some(release_toml.to_owned());
    Ok(files)
}

/// The repository's real `release-plz.toml`, with a `release.toml` that builds
/// the two tools binaries, validates and tags both with the group's `tools-v`.
#[test]
fn the_real_release_plz_toml_validates_with_a_tools_release() -> TestResult {
    let release =
        "[[binaries]]\npackage = \"velnor-ci\"\n\n[[binaries]]\npackage = \"velnor-gen\"\n";
    let mut files = real_files(release)?;
    let members: Vec<(&str, &str)> = [
        "velnor-ci",
        "velnor-gen",
        "velnor-runner-cli",
        "velnor-resource-probe",
        "velnor-runner-attestation",
        "velnor-runner-core",
        "velnor-runner-github",
        "velnor-runner-host",
        "velnor-runner-image-manifest",
        "velnor-runner-qualification",
    ]
    .into_iter()
    .map(|name| (name, "Cargo.toml"))
    .collect();
    for with_workspace in [false, true] {
        if with_workspace {
            with_metadata(&mut files, &[("Cargo.toml", &members)]);
        }
        let plan = ReleasePlan::read(&files)?;
        assert_eq!(plan.binaries.len(), 2);
        for binary in &plan.binaries {
            assert_eq!(binary.tag_pattern.as_str(), "tools-v{{ version }}");
        }
    }
    Ok(())
}

#[test]
fn the_plan_and_the_helper_name_each_package_of_a_group_with_followers() -> TestResult {
    let release = "[[binaries]]\npackage = \"velnor-ci\"\n";
    let files = real_files(release)?;
    let text = files
        .release_plz_toml
        .get(".")
        .ok_or("no release-plz.toml")?;
    let groups = velnor_release::release_version_groups(".", text)?;
    assert_eq!(groups.get("velnor-ci").map(String::as_str), Some("tools"));
    assert_eq!(groups.get("velnor-gen").map(String::as_str), Some("tools"));
    assert_eq!(
        groups.get("velnor-runner-cli").map(String::as_str),
        Some("runner")
    );
    assert_eq!(ReleasePlan::read(&files)?.roots[0].version_groups, groups);
    let ungrouped = plz(&[package("solo", TOOLS, None, "")]);
    assert!(velnor_release::release_version_groups(".", &ungrouped)?.is_empty());
    Ok(())
}
