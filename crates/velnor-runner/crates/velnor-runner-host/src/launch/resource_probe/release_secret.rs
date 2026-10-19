//! Host-only release-reader secret loaded from the validated Keychain reference.

use std::str;

use zeroize::Zeroizing;

use crate::config::{HostConfig, KeychainReference};
use crate::error::HostError;
use crate::keychain::load_secret;

/// A release-reader token held only by the host controller and provider path.
pub(crate) struct ReleaseReadSecret(Zeroizing<Vec<u8>>);

impl ReleaseReadSecret {
    #[cfg(test)]
    pub(crate) fn test_inert() -> Self {
        Self(Zeroizing::new(Vec::new()))
    }

    /// Load the dedicated host credential after a check of its configured role.
    ///
    /// # Errors
    ///
    /// Gives `HostError::Config` when the reference reuses a repository credential.
    /// Gives `HostError::Keychain` when no reference is configured or the Keychain item is unavailable.
    /// Gives `HostError::EmptySecret` when loaded bytes contain no token.
    /// Gives `HostError::Keychain` when token bytes contain invalid UTF-8, whitespace, or control bytes.
    pub(crate) fn load(config: &HostConfig) -> Result<Self, HostError> {
        let reference = validated_reference(config)?;
        let bytes = load_secret(reference.service(), reference.account())?;
        token_text(&bytes)?;
        Ok(Self(bytes))
    }

    pub(in crate::launch::resource_probe) fn as_provider_token(&self) -> Result<&str, HostError> {
        token_text(&self.0)
    }
}

fn validated_reference(config: &HostConfig) -> Result<&KeychainReference, HostError> {
    config.validate()?;
    config
        .release_read_credential_ref()
        .ok_or(HostError::Keychain)
}

fn token_text(bytes: &[u8]) -> Result<&str, HostError> {
    let token = without_one_terminal_line_ending(bytes);
    if token.is_empty() {
        return Err(HostError::EmptySecret);
    }
    if token
        .iter()
        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(HostError::Keychain);
    }
    str::from_utf8(token).map_err(|_| HostError::Keychain)
}

fn without_one_terminal_line_ending(bytes: &[u8]) -> &[u8] {
    if let Some(token) = bytes.strip_suffix(b"\r\n") {
        token
    } else if let Some(token) = bytes.strip_suffix(b"\n") {
        token
    } else {
        bytes
    }
}

#[cfg(test)]
#[path = "release_secret_tests.rs"]
mod tests;
