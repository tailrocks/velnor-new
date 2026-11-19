// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitIdentity {
    name: String,
    email: String,
}

impl GitCommitIdentity {
    pub fn try_new(
        name: impl Into<String>,
        email: impl Into<String>,
    ) -> Result<Self, GitCommitIdentityError> {
        let name = name.into();
        let email = email.into();
        if !valid_name(&name) {
            return Err(GitCommitIdentityError::Name);
        }
        if !valid_email(&email) {
            return Err(GitCommitIdentityError::Email);
        }
        Ok(Self { name, email })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn email(&self) -> &str {
        &self.email
    }

    #[must_use]
    pub fn sign_off_line(&self) -> String {
        format!("Signed-off-by: {} <{}>", self.name, self.email)
    }

    pub(super) fn matches_fields(&self, name: &str, email: &str) -> bool {
        self.name == name && self.email == email
    }
}

impl Serialize for GitCommitIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut identity = serializer.serialize_struct("GitCommitIdentity", 2)?;
        identity.serialize_field("name", &self.name)?;
        identity.serialize_field("email", &self.email)?;
        identity.end()
    }
}

impl<'de> Deserialize<'de> for GitCommitIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawIdentity {
            name: String,
            email: String,
        }

        let raw = RawIdentity::deserialize(deserializer)?;
        Self::try_new(raw.name, raw.email).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GitCommitIdentityError {
    #[error("incorrect Git author name")]
    Name,

    #[error("incorrect Git author email")]
    Email,
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && value
            .chars()
            .all(|character| !character.is_control() && !matches!(character, '<' | '>'))
}

fn valid_email(value: &str) -> bool {
    if value.is_empty() || value.len() > 254 || !value.is_ascii() || value.trim() != value {
        return false;
    }
    let mut parts = value.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    if parts.next().is_some()
        || local.is_empty()
        || domain.is_empty()
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
    {
        return false;
    }
    local.bytes().all(local_character)
        && domain.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn local_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&byte)
}
