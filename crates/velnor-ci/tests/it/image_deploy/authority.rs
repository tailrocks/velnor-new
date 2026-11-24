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
    ]))
}

fn run(api: &Api, actor: &str, actors: &[&str]) -> Result<String, String> {
    let actors: Vec<String> = actors.iter().map(|actor| (*actor).to_owned()).collect();
    authorize(
        api,
        &repo(),
        &Release {
            tag: "svc-v1.2.3",
            actor,
            actors: &actors,
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
    let refused: [(&str, Box<dyn Fn(&mut Value)>); 8] = [
        (
            "no deletion rule",
            Box::new(|r| r["rules"] = json!([{"type": "creation"}, {"type": "update"}])),
        ),
        (
            "no creation rule",
            Box::new(|r| r["rules"] = json!([{"type": "update"}, {"type": "deletion"}])),
        ),
        (
            "no update rule",
            Box::new(|r| r["rules"] = json!([{"type": "creation"}, {"type": "deletion"}])),
        ),
        (
            "evaluate only",
            Box::new(|r| r["enforcement"] = json!("evaluate")),
        ),
        (
            "a branch ruleset",
            Box::new(|r| r["target"] = json!("branch")),
        ),
        (
            "another tag family",
            Box::new(|r| r["conditions"]["ref_name"]["include"] = json!(["refs/tags/other-v*"])),
        ),
        (
            "the tag is excluded",
            Box::new(|r| r["conditions"]["ref_name"]["exclude"] = json!(["refs/tags/svc-v1.*"])),
        ),
        ("no conditions", Box::new(|r| r["conditions"] = Value::Null)),
    ];
    for (what, change) in refused {
        let message = run(&api(&ruleset(change)), "mallory", &["ann"])
            .err()
            .unwrap_or_default();
        assert!(
            message.contains("no active tag ruleset"),
            "{what}: {message}"
        );
    }
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
}

#[test]
fn a_listing_that_cannot_be_read_never_authorizes() {
    let mut broken = api(&ruleset(|_| {}));
    broken.0.remove(&detail());
    assert!(run(&broken, "mallory", &[]).is_err());
    let empty = Api(BTreeMap::from([(list(), json!([]))]));
    assert!(run(&empty, "mallory", &[]).is_err());
}
