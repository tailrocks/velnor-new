// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, Repo};
use velnor_ci::release::{record, root_tags};

const COMMIT: &str = "1111111111111111111111111111111111111111";
const OLDER: &str = "2222222222222222222222222222222222222222";
const OTHER: &str = "3333333333333333333333333333333333333333";
const REPO: &str = "tailrocks/velnor-new";

struct Tags(BTreeMap<String, String>);

impl Tags {
    fn new(tags: &[(&str, &str)], compares: &[(&str, &str)]) -> Self {
        let mut map = BTreeMap::new();
        for (tag, sha) in tags {
            map.insert(
                format!("/repos/{REPO}/git/ref/tags/{tag}"),
                format!(r#"{{"object":{{"type":"commit","sha":"{sha}"}}}}"#),
            );
        }
        for (sha, status) in compares {
            map.insert(
                format!("/repos/{REPO}/compare/{sha}...{COMMIT}?per_page=1"),
                format!(r#"{{"status":"{status}"}}"#),
            );
        }
        Self(map)
    }
}

impl GitHub for Tags {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| CiError::missing(format!("GET {path}: 404")))
    }
}

fn expected() -> Vec<(String, String)> {
    vec![
        ("tool".to_owned(), "v0.1.2".to_owned()),
        ("lib".to_owned(), "lib-v0.3.0".to_owned()),
    ]
}

fn state(github: &Tags) -> Result<(Vec<(String, String)>, bool), CiError> {
    let repo = Repo::parse(REPO)?;
    let found = root_tags(github, &repo, COMMIT, &expected())?;
    Ok((found.at_commit.clone(), found.done()))
}

#[test]
fn a_normal_run_with_no_tag_yet_is_pending_and_records_nothing() {
    assert_eq!(state(&Tags::new(&[], &[])), Ok((Vec::new(), false)));
}

#[test]
fn tags_of_an_earlier_release_do_not_make_the_root_done_or_fail() {
    let github = Tags::new(
        &[("v0.1.2", OLDER), ("lib-v0.3.0", OLDER)],
        &[(OLDER, "ahead")],
    );
    assert_eq!(state(&github), Ok((Vec::new(), false)));
}

#[test]
fn every_tag_at_the_verified_commit_is_done_and_recorded() {
    let github = Tags::new(&[("v0.1.2", COMMIT), ("lib-v0.3.0", COMMIT)], &[]);
    let (at_commit, done) = state(&github).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(done);
    assert_eq!(at_commit, expected());
}

#[test]
fn a_partial_failure_leaves_the_missing_root_pending_and_keeps_the_tagged_one() {
    let github = Tags::new(&[("v0.1.2", COMMIT)], &[]);
    let (at_commit, done) = state(&github).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!done);
    assert_eq!(at_commit, vec![("tool".to_owned(), "v0.1.2".to_owned())]);
}

#[test]
fn a_tag_at_another_commit_is_refused() {
    for status in ["diverged", "behind"] {
        let github = Tags::new(&[("v0.1.2", OTHER)], &[(OTHER, status)]);
        let error = state(&github).err().map(|e| e.to_string());
        let said = error.unwrap_or_default();
        assert!(
            said.contains("tag v0.1.2") && said.contains(OTHER) && said.contains(COMMIT),
            "{said}"
        );
    }
}

#[test]
fn the_recorded_json_feeds_record_beside_release_plz_output() {
    let github = Tags::new(&[("v0.1.2", COMMIT)], &[]);
    let repo = Repo::parse(REPO).unwrap_or_else(|e| unreachable!("{e}"));
    let found =
        root_tags(&github, &repo, COMMIT, &expected()).unwrap_or_else(|e| unreachable!("{e}"));
    let rest =
        r#"{"releases":[{"package_name":"lib","prs":[],"tag":"lib-v0.3.0","version":"0.3.0"}]}"#;
    let text = record(&[
        ("state.json".to_owned(), found.json()),
        ("release.json".to_owned(), rest.to_owned()),
    ]);
    assert_eq!(
        text.map_err(|e| e.to_string()),
        Ok("lib=lib-v0.3.0\ntool=v0.1.2\n".to_owned())
    );
}
