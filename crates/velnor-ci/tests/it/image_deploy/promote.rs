// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::BTreeMap;

use velnor_ci::error::CiError;
use velnor_ci::image_deploy::promote::{Promotion, Registry, promote};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const OLD: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const NEW: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const HUB: &str = "docker.io/acme/svc";
const GHCR: &str = "ghcr.io/acme/svc";

#[derive(Default)]
struct Fake {
    digests: RefCell<BTreeMap<String, String>>,
    tagged: RefCell<Vec<(String, String)>>,
}

impl Fake {
    fn with(self, reference: &str, digest: &str) -> Self {
        self.digests
            .borrow_mut()
            .insert(reference.to_owned(), digest.to_owned());
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
        Ok(())
    }
}

fn run(fake: &Fake, version: &str) -> Result<String, String> {
    let images = [HUB.to_owned(), GHCR.to_owned()];
    promote(
        fake,
        &Promotion {
            images: &images,
            version,
            commit: COMMIT,
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
    assert!(unpublished.tagged.borrow().is_empty());
    let fake = Fake::default().published();
    for version in ["1.2", "1.2.3.4", "v1.2.3", "1.2.x", "latest", "main"] {
        assert!(run(&fake, version).is_err(), "{version}");
    }
    let images = [HUB.to_owned(), HUB.to_owned()];
    let twice = promote(
        &fake,
        &Promotion {
            images: &images,
            version: "1.2.3",
            commit: COMMIT,
        },
    );
    assert!(twice.is_err());
    assert!(fake.tagged.borrow().is_empty());
}
