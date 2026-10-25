//! Validated immutable Docker image references.

use std::net::Ipv6Addr;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::errors::ContractError;

/// A Docker image reference with a SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ValidatedImageReference(String);

impl ValidatedImageReference {
    /// Parse one image reference with a SHA-256 digest.
    ///
    /// # Errors
    ///
    /// The value must use a valid Docker name and lowercase SHA-256 digest.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        if is_valid_reference(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(ContractError::config(
                "config.toml",
                "execution.profiles.job_container_image",
                "invalid_immutable_image_reference",
            ))
        }
    }

    /// Return the full image reference.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ValidatedImageReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if is_valid_reference(&value) {
            Ok(Self(value))
        } else {
            Err(de::Error::custom(
                "Image reference must use a valid name and lowercase SHA-256 digest.",
            ))
        }
    }
}

fn is_valid_reference(value: &str) -> bool {
    let Some((name_and_tag, digest)) = value.split_once("@sha256:") else {
        return false;
    };
    if name_and_tag.is_empty()
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || name_and_tag.contains('@')
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return false;
    }

    let final_slash = name_and_tag.rfind('/');
    let (name, tag) = match name_and_tag.rfind(':') {
        Some(index) if final_slash.is_none_or(|slash| index > slash) => {
            (&name_and_tag[..index], Some(&name_and_tag[index + 1..]))
        }
        _ => (name_and_tag, None),
    };
    if name.is_empty() || name.len() > 255 || tag.is_some_and(|value| !is_valid_tag(value)) {
        return false;
    }

    let components: Vec<_> = name.split('/').collect();
    if components.iter().any(|component| component.is_empty()) {
        return false;
    }
    let has_registry = components.len() > 1 && is_registry_component(components[0]);
    let path = if has_registry {
        if !is_valid_registry(components[0]) || components.len() < 2 {
            return false;
        }
        &components[1..]
    } else {
        &components[..]
    };
    !path.is_empty()
        && path.join("/").len() <= 255
        && path
            .iter()
            .all(|component| is_valid_repository_component(component))
}

fn is_registry_component(value: &str) -> bool {
    value == "localhost"
        || value.contains('.')
        || value.contains(':')
        || value.starts_with('[')
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
}

fn is_valid_registry(value: &str) -> bool {
    if let Some(rest) = value.strip_prefix('[') {
        let Some((address, suffix)) = rest.split_once(']') else {
            return false;
        };
        if !address
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b':')
            || Ipv6Addr::from_str(address).is_err()
        {
            return false;
        }
        return suffix.is_empty() || valid_port_suffix(suffix);
    }
    let mut parts = value.split(':');
    let host = parts.next().unwrap_or_default();
    let port = parts.next();
    if parts.next().is_some() || host.is_empty() || port.is_some_and(|part| !is_valid_port(part)) {
        return false;
    }
    host.split('.').all(is_valid_domain_label)
}

fn valid_port_suffix(value: &str) -> bool {
    value.strip_prefix(':').is_some_and(is_valid_port)
}

fn is_valid_port(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_valid_domain_label(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
}

fn is_valid_repository_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit()) {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        if bytes[index].is_ascii_lowercase() || bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        match bytes[index] {
            b'_' if bytes.get(index + 1) == Some(&b'_') => index += 2,
            b'.' | b'_' => index += 1,
            b'-' => {
                while index < bytes.len() && bytes[index] == b'-' {
                    index += 1;
                }
            }
            _ => return false,
        }
        if index == bytes.len()
            || !(bytes[index].is_ascii_lowercase() || bytes[index].is_ascii_digit())
        {
            return false;
        }
        index += 1;
    }
    true
}

fn is_valid_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

#[cfg(test)]
#[path = "image_reference_tests.rs"]
mod tests;
