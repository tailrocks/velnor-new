//! `requirements.txt` policy: `uv pip install --require-hashes --no-deps`.
//!
//! Every requirement is `name==version` with at least one
//! `--hash=sha256:<64 hex>`; with `--no-deps` the file must also list the full
//! transitive closure, which `uv pip compile --generate-hashes` produces.
//! Options, URLs, VCS references, editables and includes are refused: they
//! would let the file steer the installer.

use crate::{AnsibleError, CORE_DISTRIBUTION};

/// One hash-pinned requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    /// Normalized (lower-case, `_`/`.` folded to `-`) distribution name.
    pub name: String,
    /// Exact version after `==`.
    pub version: String,
    /// Lower-case hex SHA-256 digests.
    pub hashes: Vec<String>,
}

fn normalize(name: &str) -> String {
    name.to_ascii_lowercase().replace(['_', '.'], "-")
}

fn is_hash(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn is_name(text: &str) -> bool {
    text.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn is_version(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'!' | b'-' | b'_'))
}

/// Logical requirement lines: backslash continuations joined, comments removed.
fn logical_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for raw in text.lines() {
        let line = raw.split(" #").next().unwrap_or("");
        let line = if line.trim_start().starts_with('#') {
            ""
        } else {
            line
        };
        let (body, continues) = match line.trim_end().strip_suffix('\\') {
            Some(body) => (body, true),
            None => (line.trim_end(), false),
        };
        current.push(' ');
        current.push_str(body.trim());
        if !continues {
            let joined = current.trim().to_owned();
            if !joined.is_empty() {
                lines.push(joined);
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        lines.push(current.trim().to_owned());
    }
    lines
}

/// Parse a fully pinned requirements file.
///
/// # Errors
///
/// Every finding, sorted: `python_requirement_option_forbidden`,
/// `python_requirement_unpinned:<name>`, `python_requirements_unhashed:<name>`,
/// `python_requirement_hash_malformed:<name>`, `python_requirement_duplicate:<name>`.
pub fn parse_requirements(text: &str) -> Result<Vec<Requirement>, Vec<AnsibleError>> {
    let mut parsed: Vec<Requirement> = Vec::new();
    let mut findings = Vec::new();
    for line in logical_lines(text) {
        if line.starts_with('-') || line.contains("://") || line.contains(" @ ") {
            findings.push(AnsibleError::new(
                "python_requirement_option_forbidden",
                &line,
            ));
            continue;
        }
        let mut words = line.split_whitespace();
        let spec = words.next().unwrap_or("");
        let Some((name, version)) = spec.split_once("==") else {
            findings.push(AnsibleError::new("python_requirement_unpinned", spec));
            continue;
        };
        let name = name.split('[').next().unwrap_or(name);
        if !is_name(name) || !is_version(version) {
            findings.push(AnsibleError::new("python_requirement_unpinned", spec));
            continue;
        }
        let mut hashes = Vec::new();
        for word in words {
            match word.strip_prefix("--hash=") {
                Some(hash) if is_hash(hash) => {
                    hashes.push(hash.trim_start_matches("sha256:").to_owned());
                }
                Some(_) => {
                    findings.push(AnsibleError::new("python_requirement_hash_malformed", name));
                }
                None if word.starts_with(';') => {}
                None => {
                    findings.push(AnsibleError::new(
                        "python_requirement_option_forbidden",
                        word,
                    ));
                }
            }
        }
        if hashes.is_empty() {
            findings.push(AnsibleError::new("python_requirements_unhashed", name));
        }
        let name = normalize(name);
        if parsed.iter().any(|r| r.name == name) {
            findings.push(AnsibleError::new("python_requirement_duplicate", &name));
        }
        parsed.push(Requirement {
            name,
            version: version.to_owned(),
            hashes,
        });
    }
    if findings.is_empty() {
        Ok(parsed)
    } else {
        findings.sort();
        findings.dedup();
        Err(findings)
    }
}

/// The locked `ansible-core` must be the version Mise declares (decision 4).
///
/// # Errors
///
/// `python_requirement_missing:ansible-core` when absent and
/// `conflicting_project_tool_versions:ansible-core` on disagreement.
pub fn check_core_agreement(
    requirements: &[Requirement],
    mise_ansible_core: &str,
) -> Result<(), AnsibleError> {
    let Some(core) = requirements.iter().find(|r| r.name == CORE_DISTRIBUTION) else {
        return Err(AnsibleError::new(
            "python_requirement_missing",
            CORE_DISTRIBUTION,
        ));
    };
    if core.version == mise_ansible_core {
        Ok(())
    } else {
        Err(AnsibleError::new(
            "conflicting_project_tool_versions",
            CORE_DISTRIBUTION,
        ))
    }
}
