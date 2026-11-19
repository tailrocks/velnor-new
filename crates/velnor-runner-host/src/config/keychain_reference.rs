// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::error::HostError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Source {
    Keychain { service: String, account: String },
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeychainReference(Source);

impl KeychainReference {
    pub fn parse(value: &str) -> Result<Self, HostError> {
        if let Some(path) = value.strip_prefix("file:") {
            return parse_file(path);
        }
        let Some(pair) = value.strip_prefix("keychain:") else {
            return Err(HostError::Config);
        };
        let Some((service, account)) = pair.split_once('/') else {
            return Err(HostError::Config);
        };
        if !valid_keychain_component(service)
            || !valid_keychain_component(account)
            || account.contains('/')
        {
            return Err(HostError::Config);
        }
        Ok(Self(Source::Keychain {
            service: service.to_owned(),
            account: account.to_owned(),
        }))
    }

    #[must_use]
    pub fn keychain_parts(&self) -> Option<(&str, &str)> {
        match &self.0 {
            Source::Keychain { service, account } => Some((service, account)),
            Source::File(_) => None,
        }
    }

    pub fn load(&self) -> Result<Zeroizing<Vec<u8>>, HostError> {
        match &self.0 {
            Source::Keychain { service, account } => crate::keychain::load_secret(service, account),
            Source::File(path) => crate::credential_file::load(path),
        }
    }

    pub(crate) async fn load_until(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<Zeroizing<Vec<u8>>, HostError> {
        let reference = self.clone();
        off_runtime(deadline, move || reference.load()).await
    }
}

pub(crate) async fn off_runtime<T, F>(
    deadline: tokio::time::Instant,
    load: F,
) -> Result<T, HostError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, HostError> + Send + 'static,
{
    let _ = deadline;
    load()
}

fn parse_file(path: &str) -> Result<KeychainReference, HostError> {
    let path = Path::new(path);
    let clean = cfg!(target_os = "linux")
        && path.is_absolute()
        && path.file_name().is_some()
        && !path.to_string_lossy().chars().any(char::is_control)
        && path
            .components()
            .all(|part| !matches!(part, std::path::Component::ParentDir));
    if clean {
        Ok(KeychainReference(Source::File(path.to_path_buf())))
    } else {
        Err(HostError::Config)
    }
}

impl Display for KeychainReference {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Source::Keychain { service, account } => {
                write!(formatter, "keychain:{service}/{account}")
            }
            Source::File(path) => write!(formatter, "file:{}", path.display()),
        }
    }
}

impl Serialize for KeychainReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for KeychainReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

fn valid_keychain_component(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|character| {
            character.is_whitespace() || character.is_control() || matches!(character, '/' | '\\')
        })
}

#[cfg(test)]
#[path = "keychain_reference_tests.rs"]
mod tests;
