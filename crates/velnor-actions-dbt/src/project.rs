// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::packages::{parse_packages, verify_lock};
use crate::paths::stays_inside;
use crate::yaml::{top_list, top_scalars};
use crate::{DbtError, check_profiles, uses_postgres_adapter};
use velnor_actions_contract::config::DBT_JOB_PREFIX;

const PATH_LIST_KEYS: [&str; 8] = [
    "model-paths",
    "macro-paths",
    "seed-paths",
    "test-paths",
    "snapshot-paths",
    "analysis-paths",
    "asset-paths",
    "docs-paths",
];

const PATH_KEYS: [&str; 3] = ["target-path", "log-path", "packages-install-path"];

#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectFiles<'a> {
    pub project: &'a str,

    pub packages: Option<&'a str>,

    pub lock: Option<&'a str>,

    pub profiles: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub dir: String,

    pub name: String,

    pub profile: String,

    pub postgres_adapter: bool,
}

fn is_identifier(text: &str) -> bool {
    text.bytes().next().is_some_and(|b| !b.is_ascii_digit())
        && text.len() <= 64
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[must_use]
pub fn slug(dir: &str) -> String {
    let mut out = String::new();
    for ch in dir.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
        } else if ch.is_ascii_uppercase() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let slug = out.trim_end_matches('-');
    if slug.is_empty() { "root" } else { slug }.to_owned()
}

#[must_use]
pub fn job_id(project: &Project) -> String {
    format!("{DBT_JOB_PREFIX}{}", slug(&project.dir))
}

#[must_use]
pub fn job_name(project: &Project) -> String {
    format!("dbt / {}", project.dir)
}

#[must_use]
pub fn analyze_project(dir: &str, files: &ProjectFiles<'_>) -> (Option<Project>, Vec<DbtError>) {
    let mut findings = Vec::new();
    let scalars = top_scalars(files.project);
    let name = scalars.get("name").cloned().unwrap_or_default();
    let profile = scalars.get("profile").cloned().unwrap_or_default();
    if !is_identifier(&name) {
        findings.push(DbtError::new("dbt_project_name_invalid", &name));
    }
    if !is_identifier(&profile) {
        findings.push(DbtError::new("dbt_project_profile_invalid", &profile));
    }
    for key in PATH_KEYS {
        if let Some(value) = scalars.get(key)
            && !stays_inside(value)
        {
            findings.push(DbtError::new("dbt_path_escapes_project", key));
        }
    }
    for key in PATH_LIST_KEYS {
        if let Some(items) = top_list(files.project, key)
            && items.iter().any(|item| !stays_inside(item))
        {
            findings.push(DbtError::new("dbt_path_escapes_project", key));
        }
    }
    if let Some(text) = files.packages {
        match parse_packages(text) {
            Ok(manifest) => {
                let lock = files.lock.map(parse_packages);
                match lock {
                    Some(Err(error)) => findings.push(error),
                    Some(Ok(lock)) => findings.extend(verify_lock(&manifest, Some(&lock))),
                    None => findings.extend(verify_lock(&manifest, None)),
                }
            }
            Err(error) => findings.push(error),
        }
    }
    if let Some(text) = files.profiles {
        findings.extend(check_profiles(text));
    }
    findings.sort();
    findings.dedup();
    let project = findings
        .iter()
        .all(|f| !f.code().starts_with("dbt_project_"))
        .then(|| Project {
            dir: dir.to_owned(),
            name,
            profile,
            postgres_adapter: files.profiles.is_some_and(uses_postgres_adapter),
        });
    (project, findings)
}
