//! Stack-neutral workflow-tool identity (ver §2).
//!
//! The catalog MUST identify each workflow tool with exact version,
//! source, platforms, and digest. Adapters carry these records; the
//! checker compares pins against them.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::manifest_checks::check_sha256;

/// One workflow tool's pinned identity (ver §2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    /// Registry tool name.
    pub name: String,
    /// Exact pinned version.
    pub version: String,
    /// Immutable source URL the pin was qualified from.
    pub source: String,
    /// Supported platform labels (sorted, non-empty).
    pub platforms: Vec<String>,
    /// SHA-256 of the qualified artifact (64 lowercase hex).
    pub digest: String,
}

impl ToolIdentity {
    /// Validate name, exact version, source, platforms, and digest.
    ///
    /// A validated identity is a trust record: the digest must be the
    /// SHA-256 of a qualified artifact, so the all-zero placeholder is
    /// rejected even though it is shape-valid hex (P03-8b).
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.name.trim().is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(ContractError::config(file, "tool.name", "malformed_name"));
        }
        check_exact_version(&self.version, file)?;
        check_source(&self.source, file)?;
        if self.platforms.is_empty() {
            return Err(ContractError::config(
                file,
                "tool.platforms",
                "empty_platforms",
            ));
        }
        for platform in &self.platforms {
            if platform.trim().is_empty() || platform.contains(' ') {
                return Err(ContractError::config(
                    file,
                    "tool.platforms",
                    "malformed_platform",
                ));
            }
        }
        if self.platforms.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ContractError::config(
                file,
                "tool.platforms",
                "must_be_sorted_unique",
            ));
        }
        check_sha256(&self.digest, file, "tool.digest")?;
        if self.digest.bytes().all(|byte| byte == b'0') {
            return Err(ContractError::config(
                file,
                "tool.digest",
                "placeholder_digest",
            ));
        }
        Ok(())
    }
}

/// Check an exact numeric dotted version (`X.Y.Z...`, no suffixes).
fn check_exact_version(version: &str, file: &str) -> Result<(), ContractError> {
    let parts: Vec<&str> = version.split('.').collect();
    let valid = parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(
            file,
            "tool.version",
            "inexact_version",
        ))
    }
}

/// Check an immutable `https://` source URL (no floating segments).
fn check_source(source: &str, file: &str) -> Result<(), ContractError> {
    let Some(rest) = source.strip_prefix("https://") else {
        return Err(ContractError::config(
            file,
            "tool.source",
            "mutable_or_malformed_url",
        ));
    };
    // A canonical release page may have one terminal slash. Interior empty
    // segments remain malformed; stripping every slash would hide them.
    let canonical = rest.strip_suffix('/').unwrap_or(rest);
    let valid = canonical.split_once('/').is_some_and(|(host, path)| {
        valid_source_host(host)
            && path.split('/').all(|segment| {
                !segment.is_empty()
                    && !matches!(segment, "." | "..")
                    && !segment.eq_ignore_ascii_case("latest")
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'+')
                    })
            })
    });
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(
            file,
            "tool.source",
            "mutable_or_malformed_url",
        ))
    }
}

/// Closed DNS authority grammar: no credentials, ports, escapes or empty labels.
fn valid_source_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::check_source;

    #[test]
    fn canonical_versioned_release_urls_allow_one_terminal_slash() {
        for source in [
            "https://www.python.org/downloads/release/python-3147/",
            "https://www.python.org/downloads/release/python-3147",
            "https://github.com/python/cpython/tree/v3.14.7",
            "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        ] {
            assert!(check_source(source, "catalog").is_ok(), "{source}");
        }
    }

    #[test]
    fn canonical_source_rejects_ambiguous_and_floating_urls() {
        for source in [
            "http://www.python.org/downloads/release/python-3147/",
            "https://www.python.org/downloads/release/python-3147//",
            "https://www.python.org/downloads//release/python-3147/",
            "https://www.python.org/downloads/../release/python-3147/",
            "https://www.python.org/downloads/./release/python-3147/",
            "https://www.python.org/downloads/%2e%2e/release/python-3147/",
            "https://www.python.org/downloads/release/python-3147/?next=x",
            "https://www.python.org/downloads/release/python-3147/#files",
            "https://www.python.org/downloads/release/latest/",
            "https://www.python.org/downloads/release/LATEST/",
            "https://user@www.python.org/downloads/release/python-3147/",
            "https://www.python.org:443/downloads/release/python-3147/",
            "https://www..python.org/downloads/release/python-3147/",
            "https://www.python.org/downloads\\release/python-3147/",
            "https://www.python.org/downloads/release/python-3147/\n",
            "https://www.python.org/",
        ] {
            assert!(check_source(source, "catalog").is_err(), "{source:?}");
        }
        let oversized_label = format!("https://{}.example/release/v1.2.3", "a".repeat(64));
        assert!(check_source(&oversized_label, "catalog").is_err());
        let label = "a".repeat(63);
        let oversized_host = format!("https://{label}.{label}.{label}.{label}/release/v1.2.3");
        assert!(check_source(&oversized_host, "catalog").is_err());
    }
}
