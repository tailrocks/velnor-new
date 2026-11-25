// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;
use velnor_ci::error::CiError;
use velnor_ci::image_deploy::promote::{Promotion, Registry, is_missing, live_tag_commit, promote};

use crate::support::{FakeGitHub, repo};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const OLD: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const NEW: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const HUB: &str = "docker.io/acme/svc";
const GHCR: &str = "ghcr.io/acme/svc";

#[derive(Default)]
struct Fake {
    digests: RefCell<BTreeMap<String, String>>,
    tagged: RefCell<Vec<(String, String)>>,
    racing: Option<String>,
}

impl Fake {
    fn with(self, reference: &str, digest: &str) -> Self {
        self.digests
            .borrow_mut()
            .insert(reference.to_owned(), digest.to_owned());
        self
    }

    fn racing(mut self, digest: &str) -> Self {
        self.racing = Some(digest.to_owned());
        self
    }

    fn published(self) -> Self {
        self.with(&format!("{HUB}:sha-{COMMIT}"), NEW)
            .with(&format!("{GHCR}:sha-{COMMIT}"), NEW)
    }
}

impl Registry for Fake {
    fn digest(&self, reference: &str) -> Result<Option<String>, CiError> {
        Ok(self.digests.borrow().get(reference).cloned())
    }

    fn tag(&self, source: &str, target: &str) -> Result<(), CiError> {
        self.tagged
            .borrow_mut()
            .push((source.to_owned(), target.to_owned()));
        let digest = self.racing.clone().unwrap_or_else(|| {
            source
                .split_once('@')
                .map_or_else(String::new, |(_, digest)| digest.to_owned())
        });
        self.digests.borrow_mut().insert(target.to_owned(), digest);
        Ok(())
    }
}

fn run(fake: &Fake, version: &str) -> Result<String, String> {
    run_at(fake, version, COMMIT)
}

fn run_at(fake: &Fake, version: &str, live: &str) -> Result<String, String> {
    let images = [HUB.to_owned(), GHCR.to_owned()];
    let live_commit = || Ok(live.to_owned());
    promote(
        fake,
        &Promotion {
            images: &images,
            version,
            commit: COMMIT,
            live_commit: &live_commit,
        },
    )
    .map_err(|error| error.to_string())
}

#[test]
fn a_new_version_tags_the_published_digest_in_every_registry() {
    let fake = Fake::default().published();
    assert!(run(&fake, "1.2.3").is_ok());
    assert_eq!(
        *fake.tagged.borrow(),
        [
            (format!("{HUB}@{NEW}"), format!("{HUB}:1.2.3")),
            (format!("{GHCR}@{NEW}"), format!("{GHCR}:1.2.3")),
        ]
    );
}

#[test]
fn a_version_that_already_names_the_digest_is_left_alone_and_a_rerun_completes_the_rest() {
    let rerun = Fake::default()
        .published()
        .with(&format!("{HUB}:1.2.3"), NEW);
    assert!(run(&rerun, "1.2.3").is_ok());
    assert_eq!(
        *rerun.tagged.borrow(),
        [(format!("{GHCR}@{NEW}"), format!("{GHCR}:1.2.3"))]
    );
}

#[test]
fn a_version_that_names_another_digest_is_never_moved_and_nothing_is_tagged_anywhere() {
    let fake = Fake::default()
        .published()
        .with(&format!("{GHCR}:1.2.3"), OLD);
    let message = run(&fake, "1.2.3").err().unwrap_or_default();
    assert!(
        message.contains(OLD) && message.contains(NEW) && message.contains("never moved"),
        "{message}"
    );
    assert!(fake.tagged.borrow().is_empty());
}

#[test]
fn an_unpublished_commit_and_malformed_requests_promote_nothing() {
    let unpublished = Fake::default().with(&format!("{HUB}:sha-{COMMIT}"), NEW);
    let message = run(&unpublished, "1.2.3").err().unwrap_or_default();
    assert!(message.contains("does not exist"), "{message}");
    assert!(message.contains("Re-run all jobs"), "{message}");
    assert!(unpublished.tagged.borrow().is_empty());
    let fake = Fake::default().published();
    for version in ["1.2", "1.2.3.4", "v1.2.3", "1.2.x", "latest", "main"] {
        assert!(run(&fake, version).is_err(), "{version}");
    }
    let images = [HUB.to_owned(), HUB.to_owned()];
    let live = || Ok(COMMIT.to_owned());
    let twice = promote(
        &fake,
        &Promotion {
            images: &images,
            version: "1.2.3",
            commit: COMMIT,
            live_commit: &live,
        },
    );
    assert!(twice.is_err());
    assert!(fake.tagged.borrow().is_empty());
}

#[test]
fn a_tag_that_another_writer_moved_during_the_promotion_fails_loudly_and_stops() {
    let fake = Fake::default().published().racing(OLD);
    let message = run(&fake, "1.2.3").err().unwrap_or_default();
    assert!(
        message.contains("another writer") && message.contains(OLD) && message.contains(NEW),
        "{message}"
    );
    assert_eq!(fake.tagged.borrow().len(), 1);
}

#[test]
fn only_the_registrys_own_miss_signals_count_as_a_missing_tag() {
    let reference = "ghcr.io/acme/svc:1.2.3";
    for (stderr, missing) in [
        ("ERROR: ghcr.io/acme/svc:1.2.3: not found\n", true),
        ("error: GHCR.io/acme/svc:1.2.3: Not Found", true),
        ("ERROR: MANIFEST_UNKNOWN: manifest unknown", true),
        ("ERROR: failed to authorize: manifest unknown\n", true),
        (
            "ERROR: failed to do request: Head \"https://proxy.test/v2/acme/svc/manifests/1.2.3\": 404 Not Found\n",
            false,
        ),
        ("<html><title>404 Not Found</title></html>\n", false),
        ("ERROR: ghcr.io/acme/other:1.2.3: not found\n", false),
        (
            "ERROR: proxy says ghcr.io/acme/svc:1.2.3: not found upstream\n",
            false,
        ),
        ("", false),
    ] {
        assert_eq!(is_missing(reference, stderr), missing, "{stderr}");
    }
}

#[test]
fn a_release_tag_that_moved_to_another_commit_while_the_run_waited_tags_nothing() {
    let fake = Fake::default().published();
    let other = "89abcdef0123456789abcdef0123456789abcdef";
    let message = run_at(&fake, "1.2.3", other).err().unwrap_or_default();
    assert!(message.contains("moved while the run waited"), "{message}");
    assert!(message.contains(other), "{message}");
    assert!(fake.tagged.borrow().is_empty());
}

#[test]
fn the_live_tag_commit_is_read_through_an_annotated_tag_and_refuses_anything_but_a_commit() {
    let tag = "svc-v1.2.3";
    let mut github = FakeGitHub::default();
    github.respond(
        format!("/repos/tailrocks/velnor-new/git/ref/tags/{tag}"),
        &json!({"object": {"type": "tag", "sha": "aaaa"}}).to_string(),
    );
    github.respond(
        "/repos/tailrocks/velnor-new/git/tags/aaaa".to_owned(),
        &json!({"object": {"type": "commit", "sha": COMMIT}}).to_string(),
    );
    assert_eq!(
        live_tag_commit(&github, &repo(), tag).ok().as_deref(),
        Some(COMMIT)
    );
    let mut tree = FakeGitHub::default();
    tree.respond(
        format!("/repos/tailrocks/velnor-new/git/ref/tags/{tag}"),
        &json!({"object": {"type": "tree", "sha": "bbbb"}}).to_string(),
    );
    assert!(live_tag_commit(&tree, &repo(), tag).is_err());
    assert!(live_tag_commit(&FakeGitHub::default(), &repo(), tag).is_err());
}
