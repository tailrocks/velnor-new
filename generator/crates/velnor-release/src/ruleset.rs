// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::json;

use crate::plan::ReleasePlan;
use crate::tag::PACKAGE_WILDCARD;

pub const RELEASE_APP_SLUG: &str = "velnor-release";
pub const RELEASE_APP_ID: u64 = 5_762_496;
const CREATION_RULESET: &str = "release-tags";
const IMMUTABLE_RULESET: &str = "release-tags-immutable";
const VERSION_PLACEHOLDER: &str = "{{ version }}";

#[must_use]
pub fn ruleset_json(plan: &ReleasePlan) -> String {
    let include: BTreeSet<String> = plan
        .roots
        .iter()
        .flat_map(|root| {
            root.packages
                .iter()
                .map(|(_, pattern)| pattern)
                .chain(&root.workspace_tags)
        })
        .map(|pattern| {
            format!(
                "refs/tags/{}",
                pattern
                    .as_str()
                    .replace(VERSION_PLACEHOLDER, "*")
                    .replace(PACKAGE_WILDCARD, "*")
            )
        })
        .collect();
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
    format!("{:#}\n", json!([creation, immutable]))
}
