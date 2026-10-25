use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::manifest_checks::check_sha256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    pub name: String,

    pub version: String,

    pub source: String,

    pub platforms: Vec<String>,

    pub digest: String,
}

impl ToolIdentity {
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

fn check_source(source: &str, file: &str) -> Result<(), ContractError> {
    let Some(rest) = source.strip_prefix("https://") else {
        return Err(ContractError::config(
            file,
            "tool.source",
            "mutable_or_malformed_url",
        ));
    };

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
