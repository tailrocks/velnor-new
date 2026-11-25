// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::error::CiError;
use velnor_ci::github::GitHub;
use velnor_ci::image_deploy::ghcr::preflight;

use crate::support::FakeGitHub;

const ORG: &str = "/orgs/acme/packages/container/svc";
const USER: &str = "/users/acme/packages/container/svc";

fn package(visibility: &str) -> String {
    json!({"visibility": visibility}).to_string()
}

#[test]
fn a_public_package_is_refused_in_the_organization_or_the_user_scope() {
    for path in [ORG, USER] {
        let mut github = FakeGitHub::default();
        github.respond(path.to_owned(), &package("public"));
        if path == USER {
            github.missing(ORG.to_owned());
        }
        let message = preflight(&github, "ghcr.io/acme/svc")
            .err()
            .map_or_else(String::new, |error| error.to_string());
        assert!(message.contains("public package"), "{path}: {message}");
    }
}

#[test]
fn a_private_or_internal_or_missing_package_is_accepted() {
    for visibility in ["private", "internal"] {
        let mut github = FakeGitHub::default();
        github.respond(ORG.to_owned(), &package(visibility));
        assert!(
            preflight(&github, "ghcr.io/acme/svc").is_ok(),
            "{visibility}"
        );
    }
    let mut none = FakeGitHub::default();
    none.missing(ORG.to_owned());
    none.missing(USER.to_owned());
    assert!(
        preflight(&none, "ghcr.io/acme/svc")
            .is_ok_and(|report| report.contains("does not exist yet"))
    );
}

#[test]
fn an_unreadable_answer_and_a_malformed_name_are_errors_not_a_pass() {
    struct Failing;
    impl GitHub for Failing {
        fn get(&self, _path: &str) -> Result<String, CiError> {
            Err(CiError::new("HTTP 500"))
        }
    }
    assert!(preflight(&Failing, "ghcr.io/acme/svc").is_err());
    let mut no_visibility = FakeGitHub::default();
    no_visibility.respond(ORG.to_owned(), "{}");
    assert!(preflight(&no_visibility, "ghcr.io/acme/svc").is_err());
    for name in [
        "docker.io/acme/svc",
        "ghcr.io/svc",
        "ghcr.io/Acme/svc",
        "ghcr.io//svc",
    ] {
        assert!(preflight(&FakeGitHub::default(), name).is_err(), "{name}");
    }
}
