// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::str;

use zeroize::Zeroizing;

use crate::config::{DeveloperImages, HostConfig, KeychainReference};
use crate::error::HostError;
use crate::keychain::load_secret;

pub(crate) enum ImageAuthority {
    Release(Zeroizing<Vec<u8>>),
    Developer(DeveloperImages),
}

impl ImageAuthority {
    pub(crate) fn load(config: &HostConfig) -> Result<Self, HostError> {
        config.validate()?;
        if let Some(images) = config.developer_images() {
            return Ok(Self::Developer(images.clone()));
        }
        let reference = validated_reference(config)?;
        let bytes = load_secret(reference.service(), reference.account())?;
        token_text(&bytes)?;
        Ok(Self::Release(bytes))
    }

    pub(crate) fn parse(token: &str) -> Result<Self, HostError> {
        let bytes = Zeroizing::new(token.as_bytes().to_vec());
        token_text(&bytes)?;
        Ok(Self::Release(bytes))
    }

    pub(crate) const fn developer_images(&self) -> Option<&DeveloperImages> {
        match self {
            Self::Release(_) => None,
            Self::Developer(images) => Some(images),
        }
    }

    pub(in crate::launch::resource_probe) fn as_provider_token(&self) -> Result<&str, HostError> {
        match self {
            Self::Release(bytes) => token_text(bytes),
            Self::Developer(_) => Err(HostError::Identity),
        }
    }
}

fn validated_reference(config: &HostConfig) -> Result<&KeychainReference, HostError> {
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
