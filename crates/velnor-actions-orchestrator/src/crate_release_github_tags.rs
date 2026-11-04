// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubAdmissionRead, OrchestratorError, Value, encode, internal, require};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubReleaseEvidence {
    pub id: u64,
    pub url: String,
}

pub fn read_github_release_evidence(
    repository: &str,
    tag: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<Option<GitHubReleaseEvidence>, OrchestratorError> {
    let Some(release) =
        api.get_optional(&format!("repos/{repository}/releases/tags/{}", encode(tag)))?
    else {
        return Ok(None);
    };
    let id = release["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_release_id"))?;
    let url = release["html_url"]
        .as_str()
        .ok_or_else(|| internal("crate_release_release_url"))?;
    let raw = format!("https://github.com/{repository}/releases/tag/{tag}");
    let encoded = format!(
        "https://github.com/{repository}/releases/tag/{}",
        encode(tag)
    );
    require(
        release["tag_name"].as_str() == Some(tag)
            && release["draft"].as_bool() == Some(false)
            && (url == raw || url == encoded),
        "release_identity",
    )?;
    Ok(Some(GitHubReleaseEvidence {
        id,
        url: url.to_owned(),
    }))
}

pub fn verify_immutable_tag_rules(
    repository: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    let listed = api.get(&format!(
        "repos/{repository}/rulesets?targets=tag&includes_parents=true&per_page=100"
    ))?;
    let list = listed
        .as_array()
        .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
    if list.len() >= 100 {
        return Err(internal("crate_release_tag_rules_pagination"));
    }
    let mut protected = false;
    let mut ids = std::collections::BTreeSet::new();
    for item in list {
        let id = item
            .get("id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        if !ids.insert(id) {
            return Err(internal("crate_release_tag_rules_unverified"));
        }
        let rule = api.get(&format!("repos/{repository}/rulesets/{id}"))?;
        if rule.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(internal("crate_release_tag_rules_unverified"));
        }
        let bypass = rule
            .get("bypass_actors")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let reference = rule
            .pointer("/conditions/ref_name")
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let includes = reference
            .get("include")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let excludes = reference
            .get("exclude")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        let rules = rule
            .get("rules")
            .and_then(Value::as_array)
            .ok_or_else(|| internal("crate_release_tag_rules_unverified"))?;
        if rule.get("target").and_then(Value::as_str) == Some("tag")
            && rule.get("enforcement").and_then(Value::as_str) == Some("active")
            && bypass.is_empty()
            && excludes.is_empty()
            && includes.iter().any(|value| value.as_str() == Some("~ALL"))
            && rules.iter().any(|value| {
                value.get("type").and_then(Value::as_str) == Some("update")
                    && (value.get("parameters").is_none()
                        || value
                            .pointer("/parameters/update_allows_fetch_and_merge")
                            .and_then(Value::as_bool)
                            == Some(false))
            })
            && rules
                .iter()
                .any(|value| value.get("type").and_then(Value::as_str) == Some("deletion"))
        {
            protected = true;
        }
    }
    if !protected {
        return Err(internal("crate_release_tag_rules_unverified"));
    }
    Ok(())
}
