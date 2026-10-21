//! `ansible.cfg` policy: a closed key allowlist.
//!
//! The committed file is the only configuration the Ansible job runs under
//! (`ANSIBLE_CONFIG` points at a byte-identical owned copy). Plugin, callback,
//! library, vault and connection-plugin keys would let the repository name
//! code or secrets for the job to load, so only inert settings are admitted and
//! every path stays inside the root.

use crate::AnsibleError;
use crate::paths::stays_inside;

/// `(section, key)` pairs the job admits.
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

/// Keys whose value is one or more root-relative paths.
const PATH_KEYS: [&str; 2] = ["inventory", "roles_path"];

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

/// Findings (sorted) for the text of a root's `ansible.cfg`.
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
            && value
                .split([':', ','])
                .map(str::trim)
                .any(|path| !path.is_empty() && !stays_inside(path))
        {
            findings.push(AnsibleError::new("ansible_cfg_path_escapes_root", key));
        }
    }
    findings.sort();
    findings.dedup();
    findings
}
