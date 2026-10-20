//! Dependency-integrity check (the `--locked` analogue for Gradle).
//!
//! Two committed artifacts must exist and be meaningful:
//!
//! - Gradle dependency locking: every project whose build script declares
//!   `dependencies` has a `gradle.lockfile` in its directory;
//! - dependency verification: `gradle/verification-metadata.xml` enables
//!   metadata verification and gives every artifact a SHA-256, with no
//!   catch-all trust entry.
//!
//! Jobs additionally run `--dependency-verification strict`; this module is
//! the generation-time half and reads only repository files.

use std::collections::BTreeSet;

use crate::lex::{Tok, tokenize};

/// Path of the verification metadata under a Gradle root.
pub const VERIFICATION_METADATA: &str = "gradle/verification-metadata.xml";
/// Name of a per-project dependency lockfile.
pub const LOCKFILE: &str = "gradle.lockfile";

/// A named integrity failure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegrityFinding {
    /// A project with dependencies has no `gradle.lockfile`.
    DependencyLockMissing(String),
    /// `gradle/verification-metadata.xml` is absent.
    VerificationMetadataMissing,
    /// `<verify-metadata>` is not `true`.
    VerificationDisabled,
    /// An artifact lacks a SHA-256 entry.
    VerificationNotSha256(String),
    /// A trust entry that disables verification for everything.
    VerificationTrustTooBroad,
}

impl IntegrityFinding {
    /// Stable machine-readable code.
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

/// True when the build script has a top-level-or-nested `dependencies { }`.
#[must_use]
pub fn declares_dependencies(script: &str) -> bool {
    let toks = tokenize(script);
    toks.windows(2).any(|pair| {
        matches!((&pair[0], &pair[1]), (Tok::Ident(name), Tok::Punct('{')) if name == "dependencies")
    })
}

/// Check locking for the given projects.
///
/// `projects` are `(gradle path, directory)` pairs relative to the root;
/// `read` returns a root-relative file's text; `files` lists root-relative files.
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

/// Check the verification metadata text (`None` when the file is absent).
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

/// Text of the first `<tag>text</tag>`.
fn element_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))?;
    Some(xml[start..start + end].trim().to_owned())
}

/// `(name attribute, inner text)` of every `<tag name="..">..</tag>`.
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

/// A `<trust ...>` entry without a group restriction, or a catch-all regex.
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

#[cfg(test)]
#[path = "integrity_tests.rs"]
mod tests;
