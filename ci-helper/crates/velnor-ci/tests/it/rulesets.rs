// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, Repo};
use velnor_ci::rulesets::check_tag_rulesets;

struct Fake(BTreeMap<String, String>);

impl GitHub for Fake {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| CiError::new(format!("no route {path}")))
    }
}

fn ruleset(
    id: u64,
    target: &str,
    enforcement: &str,
    rule: &[&str],
    include: &[&str],
    exclude: &[&str],
) -> (u64, String, String) {
    let rules: Vec<String> = rule
        .iter()
        .map(|r| format!("{{\"type\":\"{r}\"}}"))
        .collect();
    let quote = |items: &[&str]| {
        items
            .iter()
            .map(|i| format!("\"{i}\""))
            .collect::<Vec<_>>()
            .join(",")
    };
    (
        id,
        format!("{{\"id\":{id},\"target\":\"{target}\",\"enforcement\":\"{enforcement}\"}}"),
        format!(
            "{{\"conditions\":{{\"ref_name\":{{\"include\":[{}],\"exclude\":[{}]}}}},\"rules\":[{}]}}",
            quote(include),
            quote(exclude),
            rules.join(",")
        ),
    )
}

fn live(sets: &[(u64, String, String)]) -> Fake {
    let mut routes = BTreeMap::new();
    let listed: Vec<&str> = sets.iter().map(|(_, l, _)| l.as_str()).collect();
    routes.insert(
        "/repos/o/r/rulesets?per_page=100&page=1".to_owned(),
        format!("[{}]", listed.join(",")),
    );
    for (id, _, full) in sets {
        routes.insert(format!("/repos/o/r/rulesets/{id}"), full.clone());
    }
    Fake(routes)
}

fn check(github: &Fake, pattern: &str) -> Result<(), CiError> {
    check_tag_rulesets(
        github,
        &Repo::parse("o/r").unwrap_or_else(|e| unreachable!("{e}")),
        &[("release-plz.toml".to_owned(), pattern.to_owned())],
    )
}

const ALL: [&str; 3] = ["creation", "update", "deletion"];

#[test]
fn a_pattern_the_live_rulesets_cover_for_creation_and_immutability_passes() {
    let github = live(&[
        ruleset(
            1,
            "tag",
            "active",
            &["creation"],
            &["refs/tags/app-v*"],
            &[],
        ),
        ruleset(2, "tag", "active", &["update", "deletion"], &["~ALL"], &[]),
    ]);
    assert!(check(&github, "app-v{{ version }}").is_ok());
}

#[test]
fn a_changed_pattern_the_live_rulesets_never_saw_is_refused() {
    let github = live(&[ruleset(
        1,
        "tag",
        "active",
        &ALL,
        &["refs/tags/app-v*"],
        &[],
    )]);
    let error = check(&github, "evil-{{ version }}").map_err(|e| e.to_string());
    assert!(error.is_err_and(|m| m.contains("refs/tags/evil-*")));
}

#[test]
fn a_ruleset_that_lacks_creation_or_immutability_does_not_cover() {
    for rules in [
        &["creation"][..],
        &["update", "deletion"][..],
        &["creation", "update"][..],
    ] {
        let github = live(&[ruleset(
            1,
            "tag",
            "active",
            rules,
            &["refs/tags/app-v*"],
            &[],
        )]);
        assert!(check(&github, "app-v{{ version }}").is_err(), "{rules:?}");
    }
}

#[test]
fn an_inactive_branch_or_excluding_ruleset_does_not_cover() {
    for (target, enforcement, exclude) in [
        ("tag", "disabled", &[][..]),
        ("tag", "evaluate", &[][..]),
        ("branch", "active", &[][..]),
        ("tag", "active", &["refs/tags/app-v1*"][..]),
    ] {
        let github = live(&[ruleset(
            1,
            target,
            enforcement,
            &ALL,
            &["refs/tags/app-v*"],
            exclude,
        )]);
        assert!(
            check(&github, "app-v{{ version }}").is_err(),
            "{target} {enforcement}"
        );
    }
}

#[test]
fn an_unreadable_listing_fails_closed() {
    assert!(check(&Fake(BTreeMap::new()), "app-v{{ version }}").is_err());
}

#[test]
fn a_covering_ruleset_on_the_second_page_is_found() {
    let filler: Vec<(u64, String, String)> = (1..=100)
        .map(|id| ruleset(id, "branch", "active", &ALL, &["refs/heads/*"], &[]))
        .collect();
    let cover = ruleset(101, "tag", "active", &ALL, &["refs/tags/app-v*"], &[]);
    let mut github = live(&filler);
    let page = |sets: &[&(u64, String, String)]| {
        let listed: Vec<&str> = sets.iter().map(|(_, l, _)| l.as_str()).collect();
        format!("[{}]", listed.join(","))
    };
    github.0.insert(
        "/repos/o/r/rulesets?per_page=100&page=2".to_owned(),
        page(&[&cover]),
    );
    github
        .0
        .insert("/repos/o/r/rulesets/101".to_owned(), cover.2.clone());
    assert!(check(&github, "app-v{{ version }}").is_ok());
}
