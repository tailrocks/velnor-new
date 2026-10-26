// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::AnsibleError;
use crate::paths::stays_inside;

const ALLOWED: [(&str, &str); 14] = [
    ("defaults", "inventory"),
    ("defaults", "roles_path"),
    ("defaults", "host_key_checking"),
    ("defaults", "retry_files_enabled"),
    ("defaults", "interpreter_python"),
    ("defaults", "forks"),
    ("defaults", "gathering"),
    ("defaults", "timeout"),
    ("defaults", "nocows"),
    ("defaults", "deprecation_warnings"),
    ("defaults", "display_skipped_hosts"),
    ("defaults", "any_errors_fatal"),
    ("ssh_connection", "pipelining"),
    ("ssh_connection", "retries"),
];

const PATH_KEYS: [&str; 2] = ["inventory", "roles_path"];

fn path_problem(key: &str, value: &str) -> Option<&'static str> {
    let (delimiter, unsupported) = match key {
        "inventory" => (',', ':'),
        "roles_path" => (':', ','),
        _ => return None,
    };
    if value.contains(unsupported) {
        return Some("ansible_cfg_path_invalid");
    }
    let paths: Vec<&str> = value.split(delimiter).map(str::trim).collect();
    if paths.iter().any(|path| {
        path.is_empty()
            || !path.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/')
            })
    }) {
        return Some("ansible_cfg_path_invalid");
    }
    paths
        .iter()
        .any(|path| !stays_inside(path))
        .then_some("ansible_cfg_path_escapes_root")
}

fn strip_comment(line: &str) -> &str {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || trimmed.starts_with(';') {
        return "";
    }
    let cut = [" #", " ;"]
        .iter()
        .filter_map(|marker| trimmed.find(marker))
        .min()
        .unwrap_or(trimmed.len());
    trimmed[..cut].trim_end()
}

#[must_use]
pub fn check_ansible_cfg(text: &str) -> Vec<AnsibleError> {
    let mut findings = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = strip_comment(raw);
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            name.trim().clone_into(&mut section);
            continue;
        }
        let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) else {
            findings.push(AnsibleError::new("ansible_cfg_unparsed", line));
            continue;
        };
        let key = key.trim();
        if !ALLOWED.contains(&(section.as_str(), key)) {
            findings.push(AnsibleError::new(
                "ansible_cfg_key_forbidden",
                &format!("{section}.{key}"),
            ));
            continue;
        }
        if PATH_KEYS.contains(&key)
            && let Some(code) = path_problem(key, value)
        {
            findings.push(AnsibleError::new(code, key));
        }
    }
    findings.sort();
    findings.dedup();
    findings
}
