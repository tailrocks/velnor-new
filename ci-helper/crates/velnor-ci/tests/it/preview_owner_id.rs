// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The release owner's account id is a required input of `admit-preview` with no
//! default: without a usable one nothing is requested, and the run, the account and the
//! creation ruleset must all name exactly the configured id.

use serde_json::json;

use crate::admit_cases::put;
use crate::admit_world::{AdmitWorld, OWNER_ID, USER};
use crate::preview_admit::helper;

const ID: &str = "RELEASE_OWNER_ID";
const CREATION: &str = "/rulesets/26696396";

fn configured(id: u64) -> AdmitWorld {
    let mut world = AdmitWorld::new();
    world.vars.insert(ID.to_owned(), id.to_string());
    world
        .vars
        .insert("GITHUB_ACTOR_ID".to_owned(), id.to_string());
    put(&mut world, USER, "/id", json!(id));
    put(&mut world, CREATION, "/bypass_actors/0/actor_id", json!(id));
    world
}

#[test]
fn a_missing_owner_id_is_refused_before_any_request() {
    let mut world = AdmitWorld::new();
    world.vars.remove(ID);
    let (result, calls) = helper(&world);
    assert!(result.is_err(), "admitted without {ID}");
    assert!(calls.is_empty(), "requests without {ID}: {calls:?}");
    let message = result.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(message.contains(ID), "unclear error: {message}");
}

#[test]
fn an_unusable_owner_id_is_refused_before_any_request() {
    for value in [
        "",
        " ",
        "abc",
        "0",
        "000",
        "-4242",
        "+4242",
        " 4242",
        "4242 ",
        "4242\n",
        "42.0",
        "0x10",
        "4_242",
        "٤٢٤٢",
        "18446744073709551616",
        "9007199254740993",
    ] {
        let mut world = AdmitWorld::new();
        world.vars.insert(ID.to_owned(), value.to_owned());
        let (result, calls) = helper(&world);
        assert!(result.is_err(), "admitted {ID}={value:?}");
        assert!(calls.is_empty(), "{ID}={value:?} made requests: {calls:?}");
    }
}

#[test]
fn an_actor_id_that_is_not_the_configured_one_is_refused_before_the_account_is_read() {
    for (key, value) in [("GITHUB_ACTOR_ID", "4243"), (ID, "4243")] {
        let mut world = AdmitWorld::new();
        world.vars.insert(key.to_owned(), value.to_owned());
        let (result, calls) = helper(&world);
        assert!(result.is_err(), "admitted with {key}={value}");
        assert!(
            calls.iter().all(|c| !c.starts_with("/users/")),
            "{key}={value} reached the account: {calls:?}"
        );
    }
}

#[test]
fn an_account_or_ruleset_that_is_not_the_configured_id_is_refused() {
    let mut account = AdmitWorld::new();
    put(&mut account, USER, "/id", json!(4243));
    assert!(helper(&account).0.is_err(), "admitted another account id");
    let mut ruleset = AdmitWorld::new();
    put(
        &mut ruleset,
        CREATION,
        "/bypass_actors/0/actor_id",
        json!(4243),
    );
    assert!(helper(&ruleset).0.is_err(), "admitted another bypass actor");
}

#[test]
fn a_configured_id_admits_the_owner_that_has_it() {
    for id in [OWNER_ID, 7, 139_999, 9_007_199_254_740_991] {
        let (result, _) = helper(&configured(id));
        assert!(result.is_ok(), "{id}: {:?}", result.err());
    }
}
