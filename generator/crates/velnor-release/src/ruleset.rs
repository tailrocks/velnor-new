// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::json;

use crate::plan::ReleasePlan;

pub const RELEASE_APP_SLUG: &str = "velnor-release";
pub const RELEASE_APP_ID: u64 = 5_762_496;
const RULESET_NAME: &str = "release-tags";
const VERSION_PLACEHOLDER: &str = "{{ version }}";

#[must_use]
pub fn ruleset_json(plan: &ReleasePlan) -> String {
    let include: BTreeSet<String> = plan
        .roots
        .iter()
        .flat_map(|root| &root.packages)
        .map(|(_, pattern)| {
            format!(
                "refs/tags/{}",
                pattern.as_str().replace(VERSION_PLACEHOLDER, "*")
            )
        })
        .collect();
    let ruleset = json!({
        "name": RULESET_NAME,
        "target": "tag",
        "enforcement": "active",
        "conditions": { "ref_name": { "include": include, "exclude": [] } },
        "rules": [{ "type": "creation" }],
        "bypass_actors": [{
            "actor_id": RELEASE_APP_ID,
            "actor_type": "Integration",
            "bypass_mode": "always"
        }]
    });
    format!("{ruleset:#}\n")
}
