// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;
use velnor_ci::error::CiError;
use velnor_ci::github::{branch_path, repo_path};
use velnor_ci::image_deploy::advance::{Advancement, advance};
use velnor_ci::image_deploy::promote::Registry;

use crate::support::{FakeGitHub, repo};

const OWN: &str = "0123456789abcdef0123456789abcdef01234567";
const TIP: &str = "89abcdef0123456789abcdef0123456789abcdef";
const OWN_DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const TIP_DIGEST: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const HUB: &str = "docker.io/acme/svc";
const GHCR: &str = "ghcr.io/acme/svc";

#[derive(Default)]
struct Fake {
    digests: RefCell<BTreeMap<String, String>>,
    tagged: RefCell<Vec<String>>,
    failing: Option<String>,
    racing: Option<String>,
}

impl Fake {
    fn with(self, reference: &str, digest: &str) -> Self {
        self.digests
            .borrow_mut()
            .insert(reference.to_owned(), digest.to_owned());
        self
    }

    fn tip_published(self) -> Self {
        self.with(&format!("{HUB}:sha-{TIP}"), TIP_DIGEST)
            .with(&format!("{GHCR}:sha-{TIP}"), TIP_DIGEST)
    }
}

impl Registry for Fake {
    fn digest(&self, reference: &str) -> Result<Option<String>, CiError> {
        Ok(self.digests.borrow().get(reference).cloned())
    }

    fn tag(&self, source: &str, target: &str) -> Result<(), CiError> {
        if self.failing.as_deref() == Some(target) {
            return Err(CiError::new("registry unavailable"));
        }
        self.tagged.borrow_mut().push(target.to_owned());
        let digest = match &self.racing {
            Some(other) if target.starts_with(GHCR) => other.clone(),
            _ => source
                .split_once('@')
                .map_or_else(String::new, |(_, digest)| digest.to_owned()),
        };
        self.digests.borrow_mut().insert(target.to_owned(), digest);
        Ok(())
    }
}

fn github(tip: &str) -> FakeGitHub {
    let mut fake = FakeGitHub::default();
    fake.respond(
        repo_path(&repo()),
        &json!({"id": 1, "default_branch": "main"}).to_string(),
    );
    fake.respond(
        branch_path(&repo(), "main"),
        &json!({"commit": {"sha": tip}}).to_string(),
    );
    fake
}

fn run(fake: &Fake, tip: &str) -> Result<(bool, String), String> {
    let images = [HUB.to_owned(), GHCR.to_owned()];
    advance(
        &github(tip),
        &repo(),
        fake,
        &Advancement {
            images: &images,
            branch: "main",
            commit: OWN,
            digest: OWN_DIGEST,
        },
    )
    .map(|done| (done.moved, done.report))
    .map_err(|error| error.to_string())
}

#[test]
fn the_tip_moves_the_branch_tag_to_the_digest_it_pushed_in_every_registry() {
    let fake = Fake::default();
    let done = run(&fake, OWN);
    assert!(done.is_ok_and(|(moved, _)| moved));
    assert_eq!(
        fake.tagged.borrow().clone(),
        vec![format!("{HUB}:main"), format!("{GHCR}:main")]
    );
    assert_eq!(
        fake.digests.borrow().get(&format!("{GHCR}:main")),
        Some(&OWN_DIGEST.to_owned())
    );
}

#[test]
fn a_run_that_is_not_the_tip_points_the_branch_tag_at_the_tips_own_image() {
    let fake = Fake::default()
        .tip_published()
        .with(&format!("{HUB}:main"), OWN_DIGEST)
        .with(&format!("{GHCR}:main"), OWN_DIGEST);
    let done = run(&fake, TIP);
    assert!(done.is_ok_and(|(moved, _)| moved));
    assert_eq!(
        fake.digests.borrow().get(&format!("{HUB}:main")),
        Some(&TIP_DIGEST.to_owned())
    );
    assert_eq!(
        fake.digests.borrow().get(&format!("{GHCR}:main")),
        Some(&TIP_DIGEST.to_owned())
    );
}

#[test]
fn a_tip_without_an_image_in_some_registry_moves_nothing_anywhere() {
    let fake = Fake::default().with(&format!("{HUB}:sha-{TIP}"), TIP_DIGEST);
    let done = run(&fake, TIP);
    assert!(done.is_ok_and(|(moved, report)| !moved && report.contains("nothing moved")));
    assert!(fake.tagged.borrow().is_empty());
}

#[test]
fn a_branch_tag_that_already_names_the_digest_is_left_alone() {
    let fake = Fake::default()
        .with(&format!("{HUB}:main"), OWN_DIGEST)
        .with(&format!("{GHCR}:main"), OWN_DIGEST);
    assert!(run(&fake, OWN).is_ok_and(|(moved, _)| moved));
    assert!(fake.tagged.borrow().is_empty());
}

#[test]
fn one_registry_that_fails_fails_the_job_after_the_others_were_tried_and_says_a_rerun_repairs_it() {
    let fake = Fake {
        failing: Some(format!("{GHCR}:main")),
        ..Fake::default()
    };
    let message = run(&fake, OWN).err().unwrap_or_default();
    assert!(message.contains("re-run this job"), "{message}");
    assert!(message.contains(&format!("{GHCR}:main")), "{message}");
    assert_eq!(fake.tagged.borrow().clone(), vec![format!("{HUB}:main")]);
}

#[test]
fn a_registry_that_names_another_digest_after_the_create_fails_the_job() {
    let fake = Fake {
        racing: Some(TIP_DIGEST.to_owned()),
        ..Fake::default()
    };
    let message = run(&fake, OWN).err().unwrap_or_default();
    assert!(message.contains("names sha256:2222"), "{message}");
    assert!(message.contains("re-run this job"), "{message}");
}

#[test]
fn a_digest_a_branch_or_a_commit_that_cannot_be_valid_is_refused_before_any_request() {
    let images = [HUB.to_owned()];
    for (commit, digest, branch) in [
        ("main", OWN_DIGEST, "main"),
        (OWN, "latest", "main"),
        (OWN, OWN_DIGEST, "a/b"),
        (OWN, OWN_DIGEST, ""),
    ] {
        let fake = Fake::default();
        let none = FakeGitHub::default();
        let outcome = advance(
            &none,
            &repo(),
            &fake,
            &Advancement {
                images: &images,
                branch,
                commit,
                digest,
            },
        );
        assert!(outcome.is_err(), "{commit} {digest} {branch}");
    }
}

struct Moving<'a> {
    inner: FakeGitHub,
    reads: &'a std::cell::Cell<u32>,
}

impl velnor_ci::github::GitHub for Moving<'_> {
    fn get(&self, path: &str) -> Result<String, CiError> {
        if path == branch_path(&repo(), "main") {
            self.reads.set(self.reads.get() + 1);
            if self.reads.get() > 1 {
                return Ok(json!({"commit": {"sha": TIP}}).to_string());
            }
        }
        self.inner.get(path)
    }
}

#[test]
fn a_tip_that_moves_while_the_tag_is_written_stops_the_write_and_says_so() {
    let fake = Fake::default();
    let reads = std::cell::Cell::new(0);
    let github = Moving {
        inner: github(OWN),
        reads: &reads,
    };
    let images = [HUB.to_owned(), GHCR.to_owned()];
    let outcome = advance(
        &github,
        &repo(),
        &fake,
        &Advancement {
            images: &images,
            branch: "main",
            commit: OWN,
            digest: OWN_DIGEST,
        },
    );
    let message = outcome
        .err()
        .map_or_else(String::new, |error| error.to_string());
    assert!(message.contains("moved from"), "{message}");
    assert!(
        fake.tagged.borrow().is_empty(),
        "nothing was written under a stale tip"
    );
}
