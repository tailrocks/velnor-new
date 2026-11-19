// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_ci::error::CiError;
use velnor_ci::github::tag_ref_path;
use velnor_ci::release::{PlainName, recorded};
use velnor_ci::release_published::{Registry, check_published_tagged};
use velnor_release::ExpectedRelease;

use crate::release_record::NOTHING;
use crate::support::{FakeGitHub, repo};

#[derive(Default)]
struct FakeRegistry(BTreeSet<(String, String)>);

impl Registry for FakeRegistry {
    fn is_published(&self, package: &str, version: &str) -> Result<bool, CiError> {
        Ok(self.0.contains(&(package.to_owned(), version.to_owned())))
    }
}

fn app() -> ExpectedRelease {
    ExpectedRelease {
        package: "app".to_owned(),
        version: "1.2.3".to_owned(),
        tag: "app-v1.2.3".to_owned(),
        publishes_to_crates_io: true,
    }
}

fn published() -> FakeRegistry {
    FakeRegistry(BTreeSet::from([("app".to_owned(), "1.2.3".to_owned())]))
}

fn check(
    github: &FakeGitHub,
    registry: &FakeRegistry,
    expected: &[ExpectedRelease],
    released: &BTreeMap<PlainName, PlainName>,
) -> Result<(), String> {
    check_published_tagged(github, &repo(), registry, expected, released).map_err(|e| e.to_string())
}

fn nothing_released() -> BTreeMap<PlainName, PlainName> {
    recorded(&[("app.json".to_owned(), NOTHING.to_owned())]).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_published_crate_with_no_tag_that_release_plz_skipped_is_refused() {
    let mut github = FakeGitHub::default();
    github.missing(tag_ref_path(&repo(), "app-v1.2.3"));
    let Err(said) = check(&github, &published(), &[app()], &nothing_released()) else {
        unreachable!("a published crate with no tag must be refused");
    };
    assert!(said.contains("app 1.2.3 is already on crates.io"), "{said}");
    assert!(said.contains("app-v1.2.3"), "{said}");
    assert!(said.contains("git tag app-v1.2.3"), "{said}");
    assert!(said.contains("bump the version"), "{said}");
}

#[test]
fn a_published_crate_whose_tag_exists_is_not_refused() {
    let mut github = FakeGitHub::default();
    github.respond(
        tag_ref_path(&repo(), "app-v1.2.3"),
        r#"{"object":{"type":"commit","sha":"1111111111111111111111111111111111111111"}}"#,
    );
    assert_eq!(
        check(&github, &published(), &[app()], &nothing_released()),
        Ok(())
    );
}

#[test]
fn an_unpublished_crate_or_one_release_plz_released_is_not_refused() {
    let github = FakeGitHub::default();
    assert_eq!(
        check(
            &github,
            &FakeRegistry::default(),
            &[app()],
            &nothing_released()
        ),
        Ok(())
    );
    let released = BTreeMap::from([(
        PlainName::new("package", "app").unwrap_or_else(|e| unreachable!("{e}")),
        PlainName::tag("app-v1.2.3").unwrap_or_else(|e| unreachable!("{e}")),
    )]);
    assert_eq!(check(&github, &published(), &[app()], &released), Ok(()));
    let private = ExpectedRelease {
        publishes_to_crates_io: false,
        ..app()
    };
    assert_eq!(
        check(&github, &published(), &[private], &nothing_released()),
        Ok(())
    );
}
