// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::github::{tag_object_path, tag_ref_path};
use velnor_ci::release::PlainName;
use velnor_ci::release_commit::tagged_commits;

use crate::support::{FakeGitHub, repo};

const MERGE: &str = "1111111111111111111111111111111111111111";
const PR_HEAD: &str = "2222222222222222222222222222222222222222";
const TAG_OBJECT: &str = "3333333333333333333333333333333333333333";
const OTHER: &str = "4444444444444444444444444444444444444444";

fn tags(items: &[(&str, &str)]) -> BTreeMap<PlainName, PlainName> {
    items
        .iter()
        .map(|(package, tag)| {
            (
                PlainName::new("package", package).unwrap_or_else(|e| unreachable!("{e}")),
                PlainName::tag(tag).unwrap_or_else(|e| unreachable!("{e}")),
            )
        })
        .collect()
}

fn reference(kind: &str, sha: &str) -> String {
    format!(r#"{{"object":{{"type":"{kind}","sha":"{sha}"}}}}"#)
}

fn run(fake: &FakeGitHub, items: &[(&str, &str)]) -> Result<String, String> {
    tagged_commits(fake, &repo(), MERGE, &tags(items)).map_err(|e| e.to_string())
}

fn lightweight(fake: &mut FakeGitHub, tag: &str, commit: &str) {
    fake.respond(tag_ref_path(&repo(), tag), &reference("commit", commit));
}

#[test]
fn a_tag_on_the_verified_commit_itself_is_recorded() {
    let mut fake = FakeGitHub::default();
    lightweight(&mut fake, "app-v1.0.0", MERGE);
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0")]),
        Ok(format!("app_commit={MERGE}\n"))
    );
}

#[test]
fn an_annotated_tag_is_followed_to_the_commit_it_names() {
    let mut fake = FakeGitHub::default();
    fake.respond(
        tag_ref_path(&repo(), "app-v1.0.0"),
        &reference("tag", TAG_OBJECT),
    );
    fake.respond(
        tag_object_path(&repo(), TAG_OBJECT),
        &reference("commit", MERGE),
    );
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0")]),
        Ok(format!("app_commit={MERGE}\n"))
    );
}

#[test]
fn a_tag_on_any_commit_other_than_the_verified_one_is_refused() {
    for other in [PR_HEAD, OTHER] {
        let mut fake = FakeGitHub::default();
        lightweight(&mut fake, "app-v1.0.0", other);
        assert_eq!(
            run(&fake, &[("app", "app-v1.0.0")]),
            Err(format!(
                "tag app-v1.0.0 is on {other}, not on the verified commit {MERGE}; release pull requests must be squash-merged (or merged through a merge queue with squash) so the tag names the verified commit"
            ))
        );
    }
}

#[test]
fn an_annotated_tag_on_another_commit_is_refused() {
    let mut fake = FakeGitHub::default();
    fake.respond(
        tag_ref_path(&repo(), "app-v1.0.0"),
        &reference("tag", TAG_OBJECT),
    );
    fake.respond(
        tag_object_path(&repo(), TAG_OBJECT),
        &reference("commit", PR_HEAD),
    );
    assert!(run(&fake, &[("app", "app-v1.0.0")]).is_err());
}

#[test]
fn a_tag_that_does_not_name_a_commit_is_refused() {
    let mut fake = FakeGitHub::default();
    lightweight(&mut fake, "app-v1.0.0", OTHER);
    fake.respond(
        tag_ref_path(&repo(), "app-v1.0.0"),
        &reference("tree", OTHER),
    );
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0")]),
        Err("tag app-v1.0.0 names a tree, not a commit".to_owned())
    );
}

#[test]
fn a_missing_tag_or_a_chain_of_tags_is_refused() {
    let mut fake = FakeGitHub::default();
    fake.missing(tag_ref_path(&repo(), "app-v1.0.0"));
    assert!(run(&fake, &[("app", "app-v1.0.0")]).is_err());
    let mut fake = FakeGitHub::default();
    fake.respond(
        tag_ref_path(&repo(), "app-v1.0.0"),
        &reference("tag", TAG_OBJECT),
    );
    fake.respond(
        tag_object_path(&repo(), TAG_OBJECT),
        &reference("tag", TAG_OBJECT),
    );
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0")]),
        Err("tag app-v1.0.0 points at a tag that points at a tag, not at a commit".to_owned())
    );
}

#[test]
fn a_sha_that_is_not_forty_lowercase_hex_digits_is_refused() {
    let mut fake = FakeGitHub::default();
    lightweight(&mut fake, "app-v1.0.0", "abc\nevil=1");
    assert!(run(&fake, &[("app", "app-v1.0.0")]).is_err());
    let none = FakeGitHub::default();
    assert!(tagged_commits(&none, &repo(), "main", &tags(&[("app", "app-v1.0.0")])).is_err());
}

#[test]
fn a_package_whose_name_would_shadow_another_packages_commit_output_is_refused() {
    let mut fake = FakeGitHub::default();
    lightweight(&mut fake, "app-v1.0.0", MERGE);
    lightweight(&mut fake, "app_commit-v1.0.0", MERGE);
    assert_eq!(
        run(
            &fake,
            &[("app", "app-v1.0.0"), ("app_commit", "app_commit-v1.0.0")]
        ),
        Err(
            "the commit output of app would be named app_commit, which is also a released package"
                .to_owned()
        )
    );
}

#[test]
fn nothing_released_asks_nothing_and_records_nothing() {
    let none = FakeGitHub::default();
    assert_eq!(run(&none, &[]), Ok(String::new()));
}
