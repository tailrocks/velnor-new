// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::github::GitHub;
use velnor_ci::image_deploy::authority::{Release, authorize};

use crate::support::repo;

fn list() -> String {
    format!(
        "/repos/{}/rulesets?includes_parents=true&per_page=100&page=1",
        repo()
    )
}

const RUN: u64 = 99;

fn run_path() -> String {
    format!("/repos/{}/actions/runs/{RUN}", repo())
}

fn detail() -> String {
    format!("/repos/{}/rulesets/7", repo())
}

struct Api(BTreeMap<String, Value>);

impl GitHub for Api {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.0
            .get(path)
            .map(Value::to_string)
            .ok_or_else(|| CiError::new(format!("no recorded response for {path}")))
    }
}

fn ruleset(change: impl FnOnce(&mut Value)) -> Value {
    let mut detail = json!({
        "id": 7,
        "name": "release tags",
        "target": "tag",
        "enforcement": "active",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00.261+07:00",
        "conditions": {"ref_name": {"include": ["refs/tags/svc-v*"], "exclude": []}},
        "rules": [{"type": "creation"}, {"type": "update"}, {"type": "deletion"}]
    });
    change(&mut detail);
    detail
}

fn api(body: &Value) -> Api {
    Api(BTreeMap::from([
        (
            list(),
            json!([{"id": 7, "target": body["target"], "enforcement": body["enforcement"]}]),
        ),
        (detail(), body.clone()),
        (run_path(), json!({"created_at": "2026-02-01T12:00:00Z"})),
    ]))
}

fn run(api: &Api, actor: &str, actors: &[&str]) -> Result<String, String> {
    run_with(api, actor, actors, Some(RUN))
}

fn run_with(
    api: &Api,
    actor: &str,
    actors: &[&str],
    run_id: Option<u64>,
) -> Result<String, String> {
    let actors: Vec<String> = actors.iter().map(|actor| (*actor).to_owned()).collect();
    authorize(
        api,
        &repo(),
        &Release {
            tag: "svc-v1.2.3",
            actor,
            actors: &actors,
            run_id,
        },
    )
    .map_err(|error| error.to_string())
}

#[test]
fn an_allowed_actor_releases_without_any_ruleset_lookup() {
    let nothing = Api(BTreeMap::new());
    assert!(run(&nothing, "Release-Bot", &["release-bot", "ann"]).is_ok());
    let message = run(&nothing, "mallory", &["ann"]).err().unwrap_or_default();
    assert!(message.contains("no recorded response"), "{message}");
}

#[test]
fn any_other_actor_needs_an_active_tag_ruleset_that_forbids_creating_moving_and_deleting_the_tag() {
    let covered = api(&ruleset(|_| {}));
    let report = run(&covered, "mallory", &[]).unwrap_or_else(|error| error);
    assert!(report.contains("release tags"), "{report}");
    let refuse = |what: &str, change: fn(&mut Value)| {
        let message = run(&api(&ruleset(change)), "mallory", &["ann"])
            .err()
            .unwrap_or_default();
        assert!(
            message.contains("no active tag ruleset"),
            "{what}: {message}"
        );
    };
    refuse("no deletion rule", |r| {
        r["rules"] = json!([{"type": "creation"}, {"type": "update"}]);
    });
    refuse("no creation rule", |r| {
        r["rules"] = json!([{"type": "update"}, {"type": "deletion"}]);
    });
    refuse("no update rule", |r| {
        r["rules"] = json!([{"type": "creation"}, {"type": "deletion"}]);
    });
    refuse("evaluate only", |r| r["enforcement"] = json!("evaluate"));
    refuse("a branch ruleset", |r| r["target"] = json!("branch"));
    refuse("another tag family", |r| {
        r["conditions"]["ref_name"]["include"] = json!(["refs/tags/other-v*"]);
    });
    refuse("the tag is excluded", |r| {
        r["conditions"]["ref_name"]["exclude"] = json!(["refs/tags/svc-v1.*"]);
    });
    refuse("no conditions", |r| r["conditions"] = Value::Null);
    refuse("an exclude class GitHub reads as a pattern", |r| {
        r["conditions"]["ref_name"]["exclude"] = json!(["refs/tags/svc-v[0-9]*"]);
    });
    refuse("an exclude brace GitHub reads as a pattern", |r| {
        r["conditions"]["ref_name"]["exclude"] = json!(["refs/tags/svc-v{1,2}.*"]);
    });
    refuse("an include with a class", |r| {
        r["conditions"]["ref_name"]["include"] = json!(["refs/tags/svc-v*", "refs/tags/[a-z]*"]);
    });
    refuse("the ruleset changed after the push", |r| {
        r["updated_at"] = json!("2026-02-01T12:00:01Z");
    });
    refuse(
        "the ruleset changed a fraction of a second after the push",
        |r| {
            r["updated_at"] = json!("2026-02-01T12:00:00.001Z");
        },
    );
    refuse(
        "a local time that sorts before the push as text but is later in UTC",
        |r| {
            r["updated_at"] = json!("2026-02-01T05:00:01.000-08:00");
        },
    );
    refuse("a ruleset with no change time", |r| {
        r["updated_at"] = Value::Null;
        r["created_at"] = Value::Null;
    });
    for malformed in [
        "yesterday",
        "2026-02-01T12:00:00",
        "2026-02-01 12:00:00Z",
        "2026-13-01T12:00:00Z",
        "2026-02-01T12:00:00.+07:00",
        "2026-02-01T12:00:00+0700",
        "2026-02-01T12:00:00+25:00",
        "2025-04-31T12:00:00Z",
        "2025-02-29T12:00:00Z",
        "1900-02-29T12:00:00Z",
        "2026-02-30T00:00:00Z",
    ] {
        let message = run(
            &api(&ruleset(|r| r["updated_at"] = json!(malformed))),
            "mallory",
            &["ann"],
        )
        .err()
        .unwrap_or_default();
        assert!(
            message.contains("no active tag ruleset"),
            "{malformed}: {message}"
        );
    }
}

#[test]
fn a_ruleset_that_predates_the_push_vouches_and_an_unknown_push_time_never_does() {
    let covered = api(&ruleset(|_| {}));
    assert!(run(&covered, "mallory", &[]).is_ok());
    let same = api(&ruleset(|r| {
        r["updated_at"] = json!("2026-02-01T12:00:00Z");
    }));
    let message = run(&same, "mallory", &[]).err().unwrap_or_default();
    assert!(
        message.contains("no active tag ruleset"),
        "a change in the same second is not older: {message}"
    );
    for earlier in [
        "2026-02-01T18:59:59.999+07:00",
        "2026-02-01T11:59:59.999Z",
        "2026-02-01T07:30:00-04:00",
    ] {
        let local = api(&ruleset(|r| r["updated_at"] = json!(earlier)));
        assert!(run(&local, "mallory", &[]).is_ok(), "{earlier}");
    }
    let leap = api(&ruleset(|r| {
        r["updated_at"] = json!("2024-02-29T12:00:00Z")
    }));
    assert!(
        run(&leap, "mallory", &[]).is_ok(),
        "a real leap day is a valid, earlier instant"
    );
    for same_instant in ["2026-02-01T19:00:00.000+07:00", "2026-02-01T12:00:00.000Z"] {
        let local = api(&ruleset(|r| r["updated_at"] = json!(same_instant)));
        assert!(run(&local, "mallory", &[]).is_err(), "{same_instant}");
    }
    let unknown = run_with(&covered, "mallory", &[], None)
        .err()
        .unwrap_or_default();
    assert!(unknown.contains("no active tag ruleset"), "{unknown}");
    assert!(run_with(&covered, "ann", &["ann"], None).is_ok());
    let mut hidden = api(&ruleset(|_| {}));
    hidden.0.remove(&run_path());
    assert!(run(&hidden, "mallory", &[]).is_err());
}

#[test]
fn ref_patterns_follow_ruleset_globs() {
    for (pattern, covers) in [
        ("~ALL", true),
        ("refs/tags/svc-v1.2.3", true),
        ("refs/tags/**", true),
        ("refs/tags/svc-v?.2.3", true),
        ("refs/tags/*", true),
        ("refs/heads/*", false),
        ("refs/tags/svc", false),
        ("refs/tags/svc-v1.2", false),
    ] {
        let detail = ruleset(|r| r["conditions"]["ref_name"]["include"] = json!([pattern]));
        assert_eq!(
            run(&api(&detail), "mallory", &[]).is_ok(),
            covers,
            "{pattern}"
        );
    }
    for (exclude, covers) in [
        ("refs/tags/other-*", true),
        ("refs/tags/svc-v[0-9]*", false),
        ("refs/tags/svc-v{1,2}.*", false),
        ("refs/tags/svc-v!1*", false),
        ("", false),
    ] {
        let detail = ruleset(|r| r["conditions"]["ref_name"]["exclude"] = json!([exclude]));
        assert_eq!(
            run(&api(&detail), "mallory", &[]).is_ok(),
            covers,
            "exclude {exclude}"
        );
    }
}

#[test]
fn a_listing_that_cannot_be_read_never_authorizes() {
    let mut broken = api(&ruleset(|_| {}));
    broken.0.remove(&detail());
    assert!(run(&broken, "mallory", &[]).is_err());
    let empty = Api(BTreeMap::from([(list(), json!([]))]));
    assert!(run(&empty, "mallory", &[]).is_err());
}
