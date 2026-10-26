// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::lex::{Tok, tokenize};

pub const VERIFICATION_METADATA: &str = "gradle/verification-metadata.xml";

pub const LOCKFILE: &str = "gradle.lockfile";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegrityFinding {
    DependencyLockMissing(String),

    VerificationMetadataMissing,

    VerificationDisabled,

    VerificationNotSha256(String),

    VerificationTrustTooBroad,
}

impl IntegrityFinding {
    #[must_use]
    pub fn code(&self) -> String {
        match self {
            Self::DependencyLockMissing(project) => {
                format!("gradle_dependency_lock_missing:{project}")
            }
            Self::VerificationMetadataMissing => "gradle_verification_metadata_missing".to_owned(),
            Self::VerificationDisabled => "gradle_verification_disabled".to_owned(),
            Self::VerificationNotSha256(artifact) => {
                format!("gradle_verification_not_sha256:{artifact}")
            }
            Self::VerificationTrustTooBroad => "gradle_verification_trust_too_broad".to_owned(),
        }
    }
}

#[must_use]
pub fn declares_dependencies(script: &str) -> bool {
    let toks = tokenize(script);
    toks.windows(2).any(|pair| {
        matches!((&pair[0], &pair[1]), (Tok::Ident(name), Tok::Punct('{')) if name == "dependencies")
    })
}

#[must_use]
pub fn check_locking(
    projects: &[(String, String)],
    files: &BTreeSet<String>,
    read: &dyn Fn(&str) -> Option<String>,
) -> Vec<IntegrityFinding> {
    let mut findings = Vec::new();
    for (path, dir) in projects {
        let join = |name: &str| {
            if dir.is_empty() {
                name.to_owned()
            } else {
                format!("{dir}/{name}")
            }
        };
        let script = ["build.gradle.kts", "build.gradle"]
            .iter()
            .find_map(|name| {
                files
                    .contains(&join(name))
                    .then(|| read(&join(name)))
                    .flatten()
            });
        if script.is_some_and(|script| declares_dependencies(&script))
            && !files.contains(&join(LOCKFILE))
        {
            findings.push(IntegrityFinding::DependencyLockMissing(path.clone()));
        }
    }
    findings
}

#[must_use]
pub fn check_verification(metadata: Option<&str>) -> Vec<IntegrityFinding> {
    let Some(xml) = metadata else {
        return vec![IntegrityFinding::VerificationMetadataMissing];
    };
    let mut findings = Vec::new();
    if element_text(xml, "verify-metadata").as_deref() != Some("true") {
        findings.push(IntegrityFinding::VerificationDisabled);
    }
    if trust_too_broad(xml) {
        findings.push(IntegrityFinding::VerificationTrustTooBroad);
    }
    for (name, body) in elements(xml, "artifact") {
        if !has_sha256(&body) {
            findings.push(IntegrityFinding::VerificationNotSha256(name));
        }
    }
    findings
}

fn has_sha256(body: &str) -> bool {
    attribute_values(body, "sha256", "value")
        .iter()
        .any(|value| value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()))
}

fn element_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))?;
    Some(xml[start..start + end].trim().to_owned())
}

fn elements(xml: &str, tag: &str) -> Vec<(String, String)> {
    let open = format!("<{tag} ");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at..];
        let Some(head_end) = after.find('>') else {
            break;
        };
        let head = &after[..head_end];
        let name = attribute(head, "name").unwrap_or_default();
        if head.ends_with('/') {
            out.push((name, String::new()));
            rest = &after[head_end..];
            continue;
        }
        let Some(end) = after.find(&close) else { break };
        out.push((name, after[head_end + 1..end].to_owned()));
        rest = &after[end + close.len()..];
    }
    out
}

fn attribute(head: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = head.find(&needle)? + needle.len();
    let end = head[start..].find('"')?;
    Some(head[start..start + end].to_owned())
}

fn attribute_values(body: &str, tag: &str, attr: &str) -> Vec<String> {
    let open = format!("<{tag} ");
    body.match_indices(&open)
        .filter_map(|(at, _)| {
            let head = &body[at..];
            attribute(&head[..head.find('>')?], attr)
        })
        .collect()
}

fn trust_too_broad(xml: &str) -> bool {
    xml.match_indices("<trust ").any(|(at, _)| {
        let head = &xml[at..];
        let Some(end) = head.find('>') else {
            return false;
        };
        let head = &head[..end];
        let group = attribute(head, "group");
        let regex = attribute(head, "regex").as_deref() == Some("true");
        group.is_none() || (regex && matches!(group.as_deref(), Some(".*" | ".+" | "^.*$")))
    })
}
