// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::image_deploy::hub::{Preflight, preflight};

use super::transport::{Fake, Reply};

const BASE: &str = "https://hub.test";
const SECRET: &str = "dckr_pat_0123456789";
const USER: &str = "deployer";
const AUTH: &str = "https://hub.test/v2/auth/token";
const REPO: &str = "https://hub.test/v2/namespaces/acme/repositories/svc";
const TAG: &str = "https://hub.test/v2/namespaces/acme/repositories/svc/tags/sha-abc";

fn hub(repository: Reply, tag: Reply) -> Fake {
    Fake::default()
        .with(
            "POST",
            AUTH,
            Reply::json(200, r#"{"access_token":"jwt-1"}"#),
        )
        .with("GET", REPO, repository)
        .with("GET", TAG, tag)
}

fn run(fake: &Fake) -> Result<Preflight, String> {
    preflight(fake, BASE, USER, SECRET, "acme/svc", "sha-abc").map_err(|error| error.to_string())
}

#[test]
fn a_private_repository_is_accepted_and_the_tag_decides_sha_exists() {
    let new = hub(
        Reply::json(200, r#"{"is_private":true}"#),
        Reply::json(404, "{}"),
    );
    let outcome = run(&new).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!outcome.sha_exists, "{}", outcome.report);
    let existing = hub(
        Reply::json(200, r#"{"is_private":true}"#),
        Reply::json(200, "{}"),
    );
    assert!(run(&existing).is_ok_and(|outcome| outcome.sha_exists));
}

#[test]
fn credentials_go_only_to_the_login_and_the_bearer_goes_only_in_a_header() {
    let fake = hub(
        Reply::json(200, r#"{"is_private":true}"#),
        Reply::json(404, "{}"),
    );
    assert!(run(&fake).is_ok());
    let seen = fake.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].method, "POST");
    assert!(seen[1..].iter().all(|request| request.method == "GET"));
    assert!(seen[0].body.contains(SECRET) && seen[0].body.contains(USER));
    assert!(seen[0].authorization.is_none());
    for request in &seen[1..] {
        assert_eq!(request.authorization.as_deref(), Some("Bearer jwt-1"));
        assert!(!request.url.contains(SECRET) && !request.body.contains(SECRET));
    }
}

#[test]
fn a_public_or_missing_repository_is_refused_before_any_tag_lookup() {
    let public = hub(
        Reply::json(200, r#"{"is_private":false}"#),
        Reply::json(404, "{}"),
    );
    let message = run(&public).err().unwrap_or_default();
    assert!(message.contains("is public"), "{message}");
    assert_eq!(public.seen().len(), 2);
    let missing = hub(Reply::json(404, "{}"), Reply::json(404, "{}"));
    let message = run(&missing).err().unwrap_or_default();
    assert!(
        message.contains("does not exist") && message.contains("create it as a private"),
        "{message}"
    );
    let odd = hub(Reply::json(500, "{}"), Reply::json(404, "{}"));
    assert!(run(&odd).err().unwrap_or_default().contains("HTTP 500"));
}

#[test]
fn a_refused_login_or_a_failed_tag_lookup_is_an_error_that_names_no_credential() {
    let denied = Fake::default().with("POST", AUTH, Reply::json(401, r#"{"detail":"bad"}"#));
    let message = run(&denied).err().unwrap_or_default();
    assert!(message.contains("HTTP 401"), "{message}");
    assert!(!message.contains(SECRET) && !message.contains("jwt"));
    let no_token = Fake::default().with("POST", AUTH, Reply::json(200, "{}"));
    assert!(
        run(&no_token)
            .err()
            .unwrap_or_default()
            .contains("without an access token")
    );
    let broken = hub(
        Reply::json(200, r#"{"is_private":true}"#),
        Reply::json(503, "{}"),
    );
    assert!(
        run(&broken)
            .err()
            .unwrap_or_default()
            .contains("tag lookup with HTTP 503")
    );
}

#[test]
fn a_repository_tag_or_credential_that_cannot_be_valid_is_refused_without_a_request() {
    let fake = Fake::default();
    for (repository, tag, user, secret) in [
        ("acme", "sha-abc", USER, SECRET),
        ("acme/a/b", "sha-abc", USER, SECRET),
        ("Acme/svc", "sha-abc", USER, SECRET),
        ("acme/svc", "sha abc", USER, SECRET),
        ("acme/svc", "", USER, SECRET),
        ("acme/svc", "sha-abc", "", SECRET),
        ("acme/svc", "sha-abc", USER, ""),
    ] {
        assert!(preflight(&fake, BASE, user, secret, repository, tag).is_err());
    }
    assert!(fake.seen().is_empty());
}
