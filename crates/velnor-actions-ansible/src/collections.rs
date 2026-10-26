// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::AnsibleCollection;

use crate::AnsibleError;

pub const GALAXY_HOST: &str = "galaxy.ansible.com";

#[must_use]
pub fn collection_url(collection: &AnsibleCollection) -> String {
    let (namespace, name) = collection
        .name
        .split_once('.')
        .unwrap_or((&collection.name, ""));
    format!(
        "https://{GALAXY_HOST}/api/v3/plugin/ansible/content/published/collections/artifacts/{namespace}-{name}-{}.tar.gz",
        collection.version
    )
}

pub fn parse_requirements_yaml(text: &str) -> Result<BTreeMap<String, String>, AnsibleError> {
    let mut found = BTreeMap::new();
    let mut in_list = false;
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut flush = |name: &mut Option<String>, version: &mut Option<String>| {
        let Some(n) = name.take() else {
            return Ok(());
        };
        match version.take() {
            Some(v) => {
                found.insert(n, v);
                Ok(())
            }
            None => Err(AnsibleError::new("ansible_collection_unversioned", &n)),
        }
    };
    for raw in text.lines() {
        let line = raw.split(" #").next().unwrap_or("").trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if indent == 0 {
            flush(&mut name, &mut version)?;
            in_list = trimmed == "collections:";
            continue;
        }
        if !in_list {
            continue;
        }
        let entry = if let Some(rest) = trimmed.strip_prefix("- ") {
            flush(&mut name, &mut version)?;
            rest
        } else {
            trimmed
        };
        let Some((key, value)) = entry.split_once(':') else {
            return Err(AnsibleError::new("ansible_requirements_unparsed", trimmed));
        };
        let value = value.trim().trim_matches(['"', '\'']).to_owned();
        match key.trim() {
            "name" => name = Some(value),
            "version" => version = Some(value),
            _ => {}
        }
    }
    flush(&mut name, &mut version)?;
    Ok(found)
}

#[must_use]
pub fn verify_collections(
    declared: &[AnsibleCollection],
    requirements: &BTreeMap<String, String>,
) -> Vec<AnsibleError> {
    let mut findings = Vec::new();
    for collection in declared {
        if requirements.get(&collection.name) != Some(&collection.version) {
            findings.push(AnsibleError::new(
                "ansible_collections_mismatch",
                &collection.name,
            ));
        }
    }
    for name in requirements.keys() {
        if !declared.iter().any(|c| &c.name == name) {
            findings.push(AnsibleError::new("ansible_collections_mismatch", name));
        }
    }
    findings.sort();
    findings.dedup();
    findings
}
