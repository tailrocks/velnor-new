// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The login of the account whose push is admitted comes from the run's triggering
//! actor: it is validated before it is put in a path, and the transport reads only that
//! account.

use serde_json::json;
use velnor_ci::preview::login::Login;
use velnor_ci::preview::net::api_allowed;

use crate::admit_world::{AdmitWorld, OWNER_ID, USER};
use crate::preview_admit::helper;
use crate::preview_world::R;

/// The world of an owner whose login is `login`, on the same configured account id.
pub(crate) fn owned_by(login: &str) -> AdmitWorld {
    let mut world = AdmitWorld::new();
    world.api.remove(USER);
    login.clone_into(&mut world.owner);
    world.api.insert(
        format!("/users/{login}"),
        json!({"id": OWNER_ID, "login": login}),
    );
    world
        .vars
        .insert("GITHUB_TRIGGERING_ACTOR".to_owned(), login.to_owned());
    world
}

#[test]
fn the_helper_asks_for_the_triggering_actor_and_for_no_one_else() {
    for login in ["octo-owner", "Mona-Lisa-9", "a", &"x".repeat(39)] {
        let (result, calls) = helper(&owned_by(login));
        assert!(result.is_ok(), "{login}: {:?}", result.err());
        let users: Vec<&String> = calls.iter().filter(|c| c.starts_with("/users/")).collect();
        assert_eq!(users, [&format!("/users/{login}")], "{login}");
    }
}

#[test]
fn an_invalid_triggering_actor_is_refused_without_a_user_request() {
    let long = "x".repeat(40);
    for login in [
        "",
        "-octo",
        "octo--owner",
        "octo_owner",
        "octo owner",
        "octo/owner",
        "../repos/x",
        "octo-owner/",
        "octo\n",
        "octo.owner",
        "ünï",
        long.as_str(),
    ] {
        let mut world = AdmitWorld::new();
        world
            .vars
            .insert("GITHUB_TRIGGERING_ACTOR".to_owned(), login.to_owned());
        let (result, calls) = helper(&world);
        assert!(result.is_err(), "admitted {login:?}");
        assert!(
            calls.iter().all(|c| c.starts_with(R)),
            "{login:?} reached the account path: {calls:?}"
        );
    }
}

#[test]
fn the_account_must_be_the_pinned_id_under_the_actors_own_login() {
    let mut other_id = owned_by("octo-owner");
    other_id.set("/users/octo-owner", json!({"id": 1, "login": "octo-owner"}));
    assert!(helper(&other_id).0.is_err());
    let mut renamed = owned_by("octo-owner");
    renamed.set(
        "/users/octo-owner",
        json!({"id": OWNER_ID, "login": "someone"}),
    );
    assert!(helper(&renamed).0.is_err());
}

#[test]
fn login_accepts_github_logins_only() {
    for ok in ["a", "A1", "octo-owner", "a-b-c", &"x".repeat(39)] {
        assert_eq!(
            Login::parse(ok).map(|l| l.path()).ok(),
            Some(format!("/users/{ok}"))
        );
    }
    for bad in [
        "",
        "-a",
        "a--b",
        "a_b",
        "a b",
        "a/b",
        "a%2fb",
        "a?b",
        "a#b",
        "é",
        &"x".repeat(40),
    ] {
        assert!(Login::parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_transport_allows_the_repository_and_the_one_actor_account() {
    let actor = Login::parse("octo-owner").unwrap_or_else(|e| unreachable!("{e}"));
    let repo = R.to_owned();
    for path in [repo.clone(), format!("{R}/actions/runs/1")] {
        assert!(api_allowed(None, &path), "{path}");
        assert!(api_allowed(Some(&actor), &path), "{path}");
    }
    assert!(api_allowed(Some(&actor), "/users/octo-owner"));
    for path in [
        "/users/octo-owner/",
        "/users/octo-owne",
        "/users/other",
        "/users/octo-owner/repos",
        "/users/octo-owner?x=1",
        "/user",
        "/repos/other/repo",
        "/repos/tailrocks/velnor-new-x",
    ] {
        assert!(!api_allowed(Some(&actor), path), "{path}");
    }
    assert!(!api_allowed(None, "/users/octo-owner"));
}
