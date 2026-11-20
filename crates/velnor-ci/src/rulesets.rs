// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{GitHub, Repo, parse};

const PAGE: usize = 100;
const ALL_TAGS: &str = "~ALL";
const REQUIRED: &[&str] = &["creation", "update", "deletion"];

#[derive(Debug, Deserialize)]
struct Listed {
    id: u64,
    target: String,
    enforcement: String,
}

#[derive(Debug, Deserialize)]
struct Ruleset {
    #[serde(default)]
    conditions: Option<Conditions>,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
struct Conditions {
    ref_name: Option<RefName>,
}

#[derive(Debug, Deserialize)]
struct RefName {
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Rule {
    #[serde(rename = "type")]
    kind: String,
}

pub fn check_tag_rulesets(
    github: &dyn GitHub,
    repo: &Repo,
    patterns: &[(String, String)],
) -> Result<(), CiError> {
    let wanted: BTreeSet<String> = patterns
        .iter()
        .map(|(_, pattern)| velnor_release::ruleset_ref(pattern))
        .collect();
    let mut covered: Vec<(&str, BTreeSet<String>)> = REQUIRED
        .iter()
        .map(|kind| (*kind, BTreeSet::new()))
        .collect();
    for id in active_tag_rulesets(github, repo)? {
        let ruleset: Ruleset = parse(
            &github.get(&format!("/repos/{repo}/rulesets/{id}"))?,
            "ruleset",
        )?;
        let Some(RefName { include, exclude }) = ruleset.conditions.and_then(|c| c.ref_name) else {
            continue;
        };
        if !exclude.is_empty() {
            continue;
        }
        for (kind, refs) in &mut covered {
            if ruleset.rules.iter().any(|rule| rule.kind == *kind) {
                refs.extend(include.iter().cloned());
            }
        }
    }
    for (kind, refs) in &covered {
        let missing: Vec<&String> = wanted
            .iter()
            .filter(|tag| !refs.contains(*tag) && !refs.contains(ALL_TAGS))
            .collect();
        if !missing.is_empty() {
            return Err(CiError::new(format!(
                "the active tag rulesets of {repo} hold no `{kind}` rule for {missing:?}; provision the rulesets for the release tag patterns before releasing"
            )));
        }
    }
    Ok(())
}

fn active_tag_rulesets(github: &dyn GitHub, repo: &Repo) -> Result<Vec<u64>, CiError> {
    let mut ids = Vec::new();
    for page in 1.. {
        let listed: Vec<Listed> = parse(
            &github.get(&format!(
                "/repos/{repo}/rulesets?per_page={PAGE}&page={page}"
            ))?,
            "rulesets",
        )?;
        let last = listed.len() < PAGE;
        ids.extend(
            listed
                .into_iter()
                .filter(|set| set.target == "tag" && set.enforcement == "active")
                .map(|set| set.id),
        );
        if last {
            break;
        }
    }
    Ok(ids)
}
