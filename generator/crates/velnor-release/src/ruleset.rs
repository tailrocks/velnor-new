// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::plan::ReleasePlan;
use crate::tag::PACKAGE_WILDCARD;

pub const RELEASE_APP_SLUG: &str = "velnor-release";
pub const RELEASE_APP_ID: u64 = 5_762_496;
const CREATION_RULESET: &str = "release-tags";
const IMMUTABLE_RULESET: &str = "release-tags-immutable";
const VERSION_PLACEHOLDER: &str = "{{ version }}";

#[must_use]
pub fn ruleset_ref(pattern: &str) -> String {
    format!(
        "refs/tags/{}",
        pattern
            .replace(VERSION_PLACEHOLDER, "*")
            .replace(PACKAGE_WILDCARD, "*")
    )
}

#[must_use]
pub fn ruleset_refs(plan: &ReleasePlan) -> BTreeSet<String> {
    plan.roots
        .iter()
        .flat_map(|root| {
            root.packages
                .iter()
                .map(|(_, pattern)| pattern)
                .chain(&root.workspace_tags)
        })
        .map(|pattern| ruleset_ref(pattern.as_str()))
        .collect()
}

#[must_use]
pub fn ruleset_json(plan: &ReleasePlan) -> String {
    rulesets(plan)
        .iter()
        .map(|ruleset| format!("{ruleset}\n"))
        .collect()
}

#[must_use]
pub fn ruleset_drift(plan: &ReleasePlan, live: &str) -> Vec<String> {
    let Ok(Value::Array(live)) = serde_json::from_str::<Value>(live) else {
        return vec!["the live rulesets are not a JSON array".to_owned()];
    };
    rulesets(plan)
        .iter()
        .flat_map(|wanted| {
            let name = wanted["name"].as_str().unwrap_or_default();
            match live.iter().find(|found| found["name"] == wanted["name"]) {
                None => vec![format!("{name}: the ruleset does not exist")],
                Some(found) => ["target", "enforcement"]
                    .iter()
                    .filter(|key| found[**key] != wanted[**key])
                    .map(|key| format!("{name}: {key} differs"))
                    .chain(differs(
                        name,
                        "refs",
                        refs(found).as_ref(),
                        refs(wanted).as_ref(),
                    ))
                    .chain(differs(
                        name,
                        "rules",
                        rule_types(found).as_ref(),
                        rule_types(wanted).as_ref(),
                    ))
                    .chain(differs(
                        name,
                        "bypass_actors",
                        bypass(found).as_ref(),
                        bypass(wanted).as_ref(),
                    ))
                    .collect(),
            }
        })
        .collect()
}

fn differs(
    name: &str,
    what: &str,
    live: Option<&BTreeSet<String>>,
    wanted: Option<&BTreeSet<String>>,
) -> Option<String> {
    (live != wanted).then(|| {
        let show = |set: Option<&BTreeSet<String>>| {
            set.map_or_else(|| "unreadable".to_owned(), |set| format!("{set:?}"))
        };
        format!(
            "{name}: {what} differ: live {}, expected {}",
            show(live),
            show(wanted)
        )
    })
}

fn strings(value: &Value) -> Option<BTreeSet<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn refs(ruleset: &Value) -> Option<BTreeSet<String>> {
    let ref_name = &ruleset["conditions"]["ref_name"];
    let include = strings(&ref_name["include"])?;
    let exclude = strings(&ref_name["exclude"])?;
    Some(
        include
            .into_iter()
            .map(|pattern| format!("include {pattern}"))
            .chain(
                exclude
                    .into_iter()
                    .map(|pattern| format!("exclude {pattern}")),
            )
            .collect(),
    )
}

fn rule_types(ruleset: &Value) -> Option<BTreeSet<String>> {
    ruleset["rules"]
        .as_array()?
        .iter()
        .map(|rule| rule["type"].as_str().map(str::to_owned))
        .collect()
}

fn bypass(ruleset: &Value) -> Option<BTreeSet<String>> {
    ruleset["bypass_actors"]
        .as_array()?
        .iter()
        .map(|actor| {
            Some(format!(
                "{} {} {}",
                actor["actor_type"].as_str()?,
                actor["actor_id"],
                actor["bypass_mode"].as_str()?
            ))
        })
        .collect()
}

fn rulesets(plan: &ReleasePlan) -> Vec<Value> {
    let include = ruleset_refs(plan);
    let conditions = json!({ "ref_name": { "include": include, "exclude": [] } });
    let creation = json!({
        "name": CREATION_RULESET,
        "target": "tag",
        "enforcement": "active",
        "conditions": conditions,
        "rules": [{ "type": "creation" }],
        "bypass_actors": [{
            "actor_id": RELEASE_APP_ID,
            "actor_type": "Integration",
            "bypass_mode": "always"
        }]
    });
    let immutable = json!({
        "name": IMMUTABLE_RULESET,
        "target": "tag",
        "enforcement": "active",
        "conditions": conditions,
        "rules": [{ "type": "update" }, { "type": "deletion" }],
        "bypass_actors": []
    });
    vec![creation, immutable]
}
