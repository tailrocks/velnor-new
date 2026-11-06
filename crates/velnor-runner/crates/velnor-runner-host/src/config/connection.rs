// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fmt::{Display, Formatter};

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{KeychainReference, SourcePolicy};
use crate::error::HostError;

const VELNOR_LABEL: &str = "velnor";
const HOSTED_LABELS: [&str; 6] = [
    "ubuntu-22.04",
    "ubuntu-24.04",
    "ubuntu-26.04",
    "ubuntu-22.04-arm",
    "ubuntu-24.04-arm",
    "ubuntu-26.04-arm",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(String);

impl ConnectionId {
    pub fn parse(value: &str) -> Result<Self, HostError> {
        if valid_label(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(HostError::Config)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ConnectionId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ConnectionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConnectionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScaleSetSelector {
    name: String,
    labels: Vec<String>,
}

impl ScaleSetSelector {
    pub fn try_new(name: &str, labels: &[String]) -> Result<Self, HostError> {
        if !valid_label(name) || name == VELNOR_LABEL || is_hosted_label(name) {
            return Err(HostError::Config);
        }
        let mut seen = HashSet::new();
        for label in labels {
            if !valid_label(label) || is_hosted_label(label) || !seen.insert(label) {
                return Err(HostError::Config);
            }
        }
        if !seen.iter().any(|label| label.as_str() == VELNOR_LABEL)
            || !seen.iter().any(|label| label.as_str() == name)
        {
            return Err(HostError::Config);
        }
        let mut ordered = vec![VELNOR_LABEL.to_owned(), name.to_owned()];
        ordered.extend(
            labels
                .iter()
                .filter(|label| label.as_str() != VELNOR_LABEL && label.as_str() != name)
                .cloned(),
        );
        Ok(Self {
            name: name.to_owned(),
            labels: ordered,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    fn canonicalize(&mut self) -> Result<(), HostError> {
        *self = Self::try_new(&self.name, &self.labels)?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaleSetSelectorWire {
    name: String,
    labels: Vec<String>,
}

impl<'de> Deserialize<'de> for ScaleSetSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ScaleSetSelectorWire::deserialize(deserializer)?;
        Self::try_new(&wire.name, &wire.labels).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryConnection {
    pub connection_id: ConnectionId,

    pub repository_id: i64,

    pub repository: String,

    pub credential_ref: KeychainReference,

    pub scale_set: ScaleSetSelector,

    pub source_policy: SourcePolicy,
}

impl RepositoryConnection {
    pub fn try_new(
        connection_id: ConnectionId,
        repository_id: i64,
        repository: String,
        credential_ref: KeychainReference,
        scale_set: ScaleSetSelector,
        source_policy: SourcePolicy,
    ) -> Result<Self, HostError> {
        let mut connection = Self {
            connection_id,
            repository_id,
            repository,
            credential_ref,
            scale_set,
            source_policy,
        };
        connection.canonicalize()?;
        connection.validate()?;
        Ok(connection)
    }

    pub fn canonical_identity_bytes(&self) -> Result<Vec<u8>, HostError> {
        let mut connection = self.clone();
        connection.canonicalize()?;
        connection.validate()?;
        serde_json::to_vec(&connection).map_err(|_| HostError::Config)
    }

    pub(crate) fn canonicalize(&mut self) -> Result<(), HostError> {
        self.scale_set.canonicalize()?;
        self.source_policy.canonicalize();
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), HostError> {
        if self.repository_id <= 0
            || !valid_repository(&self.repository)
            || self
                .source_policy
                .pull_request_rules
                .iter()
                .any(|rule| rule.base_repository_id != self.repository_id)
        {
            return Err(HostError::Config);
        }
        self.source_policy.validate()
    }
}

fn valid_repository(repository: &str) -> bool {
    let mut parts = repository.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) => valid_repo_part(owner) && valid_repo_part(name),
        _ => false,
    }
}

fn valid_repo_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 100
        && !part.starts_with('.')
        && !part.ends_with('.')
        && !part.contains("..")
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_label(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn is_hosted_label(label: &str) -> bool {
    HOSTED_LABELS.contains(&label)
}
