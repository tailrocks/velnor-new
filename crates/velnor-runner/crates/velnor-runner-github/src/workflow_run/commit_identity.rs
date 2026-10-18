//! Explicit author identity used by controller-created nonce commits.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A validated Git commit identity with no account or machine default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitIdentity {
    name: String,
    email: String,
}

impl GitCommitIdentity {
    /// Validate an explicitly configured author name and email address.
    ///
    /// # Errors
    ///
    /// Gives [`GitCommitIdentityError`] when either value is empty or unsafe to place in a
    /// Git identity or DCO sign-off line.
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

    /// Configured author name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Configured author email.
    #[must_use]
    pub fn email(&self) -> &str {
        &self.email
    }

    /// DCO line derived from this same configured identity.
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

/// Why one explicitly configured Git identity was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GitCommitIdentityError {
    /// The author name is empty, too long, or contains a sign-off delimiter/control character.
    #[error("incorrect Git author name")]
    Name,
    /// The author email is empty, malformed, or contains a sign-off delimiter/control character.
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

#[cfg(test)]
mod tests {
    use super::{GitCommitIdentity, GitCommitIdentityError};

    #[test]
    fn sign_off_comes_from_the_validated_identity() -> Result<(), GitCommitIdentityError> {
        let identity = GitCommitIdentity::try_new("Configured Person", "person@example.com")?;
        assert_eq!(
            identity.sign_off_line(),
            "Signed-off-by: Configured Person <person@example.com>"
        );
        Ok(())
    }

    #[test]
    fn empty_or_injected_identities_are_rejected() {
        assert_eq!(
            GitCommitIdentity::try_new("", "person@example.com"),
            Err(GitCommitIdentityError::Name)
        );
        assert_eq!(
            GitCommitIdentity::try_new(
                "Person\nSigned-off-by: Other <other@example.com>",
                "person@example.com"
            ),
            Err(GitCommitIdentityError::Name)
        );
        assert_eq!(
            GitCommitIdentity::try_new(
                "Person",
                "person@example.com\nSigned-off-by: Other <other@example.com>"
            ),
            Err(GitCommitIdentityError::Email)
        );
        assert_eq!(
            GitCommitIdentity::try_new("Person", "not-an-email"),
            Err(GitCommitIdentityError::Email)
        );
    }

    #[test]
    fn serialized_identity_is_revalidated_on_read() {
        let error = serde_json::from_str::<GitCommitIdentity>(
            r#"{"name":"","email":"person@example.com"}"#,
        );
        assert!(error.is_err());
    }
}
