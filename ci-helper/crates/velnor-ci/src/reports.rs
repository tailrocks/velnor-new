// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::error::CiError;

const NAME_LIMIT: usize = 128;

#[derive(Debug, Clone, Deserialize)]
pub struct Listed {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub expired: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListedPage {
    pub total_count: usize,
    pub artifacts: Vec<Listed>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Selection {
    pub wanted: Vec<(String, u64)>,
    pub problems: Vec<String>,
}

#[must_use]
pub fn is_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_LIMIT
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub fn expected_artifacts(plan: &str) -> Result<Vec<String>, CiError> {
    serde_json::from_str::<serde_json::Value>(plan)
        .map_err(|e| CiError::new(format!("plan.json is not valid JSON: {e}")))?;
    let plan = crate::preview::json::parse(plan.as_bytes(), plan.len())
        .map_err(|_| CiError::new("plan.json repeats an object key"))?;
    let entries = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| CiError::new("plan.json has no matrix.include array"))?;
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for entry in entries {
        let name = entry
            .get("artifact_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CiError::new("plan.json matrix.include entry has no artifact_id"))?;
        if !is_file_name(name) {
            return Err(CiError::new(format!(
                "plan.json artifact_id `{name}` is not a plain file name"
            )));
        }
        if seen.insert(name) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

#[must_use]
pub fn select(expected: &[String], listed: &[Listed]) -> Selection {
    let mut live: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    let mut expired = BTreeSet::new();
    for artifact in listed {
        if artifact.expired {
            expired.insert(artifact.name.as_str());
        } else {
            live.entry(artifact.name.as_str())
                .or_default()
                .push(artifact.id);
        }
    }
    let mut wanted = Vec::new();
    let mut problems = Vec::new();
    for name in expected {
        match live.get(name.as_str()).map(Vec::as_slice) {
            Some(&[id]) => wanted.push((name.clone(), id)),
            Some(ids) => {
                let mut ids = ids.to_vec();
                ids.sort_unstable();
                let ids: Vec<_> = ids.iter().map(u64::to_string).collect();
                problems.push(format!(
                    "{name}: {} live artifacts share this name (ids {})",
                    ids.len(),
                    ids.join(", ")
                ));
            }
            None if expired.contains(name.as_str()) => problems.push(format!("{name}: expired")),
            None => problems.push(format!("{name}: not uploaded")),
        }
    }
    Selection { wanted, problems }
}
