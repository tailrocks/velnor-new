// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::github::{commit_path, tag_object_path, tag_ref_path};
use velnor_ci::release::PlainName;
use velnor_ci::release_commit::tagged_commits;

use crate::support::{FakeGitHub, repo};

const MERGE: &str = "1111111111111111111111111111111111111111";
const PR_HEAD: &str = "2222222222222222222222222222222222222222";
const TAG_OBJECT: &str = "3333333333333333333333333333333333333333";
const OTHER: &str = "4444444444444444444444444444444444444444";
const FIRST_PARENT: &str = "5555555555555555555555555555555555555555";
const TREE: &str = "6666666666666666666666666666666666666666";
const STALE_TREE: &str = "7777777777777777777777777777777777777777";

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

fn merge_of(fake: &mut FakeGitHub, parents: &[&str]) {
    let list: Vec<String> = parents
        .iter()
        .map(|p| format!(r#"{{"sha":"{p}"}}"#))
        .collect();
    fake.respond(
        commit_path(&repo(), MERGE),
        &format!(
            r#"{{"parents":[{}],"commit":{{"tree":{{"sha":"{TREE}"}}}}}}"#,
            list.join(",")
        ),
    );
    if parents.len() == 2 {
        head_tree(fake, parents[1], TREE);
    }
}

fn head_tree(fake: &mut FakeGitHub, head: &str, tree: &str) {
    fake.respond(
        commit_path(&repo(), head),
        &format!(r#"{{"parents":[],"commit":{{"tree":{{"sha":"{tree}"}}}}}}"#),
    );
}

fn run(fake: &FakeGitHub, items: &[(&str, &str)]) -> Result<String, String> {
    tagged_commits(fake, &repo(), MERGE, &tags(items)).map_err(|e| e.to_string())
}

fn lightweight(fake: &mut FakeGitHub, tag: &str, commit: &str) {
    fake.respond(tag_ref_path(&repo(), tag), &reference("commit", commit));
}

#[test]
fn a_tag_on_the_pr_head_that_the_verified_merge_commit_contains_is_recorded() {
    let mut fake = FakeGitHub::default();
    lightweight(&mut fake, "app-v1.0.0", PR_HEAD);
    merge_of(&mut fake, &[FIRST_PARENT, PR_HEAD]);
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0")]),
        Ok(format!("app_commit={PR_HEAD}\n"))
    );
}

#[test]
fn a_stale_release_pr_head_whose_tree_differs_from_the_verified_commit_is_refused() {
    let mut fake = FakeGitHub::default();
    lightweight(&mut fake, "app-v1.0.0", PR_HEAD);
    merge_of(&mut fake, &[FIRST_PARENT, PR_HEAD]);
    head_tree(&mut fake, PR_HEAD, STALE_TREE);
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0")]),
        Err(format!(
            "tag app-v1.0.0 is on the release pull request head {PR_HEAD}, whose tree differs from the verified commit {MERGE}; bring the release pull request up to date with the base branch before merging it, or squash it"
        ))
    );
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
        &reference("commit", PR_HEAD),
    );
    merge_of(&mut fake, &[FIRST_PARENT, PR_HEAD]);
    assert_eq!(
        run(&fake, &[("app", "app-v1.0.0")]),
        Ok(format!("app_commit={PR_HEAD}\n"))
    );
}

#[test]
fn a_tag_on_any_other_commit_is_refused() {
    let refused = format!(
        "tag app-v1.0.0 is on {OTHER}, which is neither the verified commit {MERGE} nor the release pull request head merged into it"
    );
    for parents in [
        vec![FIRST_PARENT, PR_HEAD],
        vec![OTHER, PR_HEAD, FIRST_PARENT],
        vec![FIRST_PARENT],
        vec![],
    ] {
        let mut fake = FakeGitHub::default();
        lightweight(&mut fake, "app-v1.0.0", OTHER);
        merge_of(&mut fake, &parents);
        assert_eq!(run(&fake, &[("app", "app-v1.0.0")]), Err(refused.clone()));
    }
}

#[test]
fn an_older_first_parent_ancestor_is_refused() {
    let mut fake = FakeGitHub::default();
    lightweight(&mut fake, "app-v1.0.0", FIRST_PARENT);
    merge_of(&mut fake, &[FIRST_PARENT, PR_HEAD]);
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
    lightweight(&mut fake, "app-v1.0.0", PR_HEAD);
    lightweight(&mut fake, "app_commit-v1.0.0", PR_HEAD);
    merge_of(&mut fake, &[FIRST_PARENT, PR_HEAD]);
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
