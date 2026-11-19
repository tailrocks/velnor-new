// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::error::HostError;

use super::KeychainReference;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReleaseReadConfig {
    credential_ref: KeychainReference,
}

impl ReleaseReadConfig {
    pub(super) const fn new(credential_ref: KeychainReference) -> Self {
        Self { credential_ref }
    }

    #[must_use]
    pub(super) const fn credential_ref(&self) -> &KeychainReference {
        &self.credential_ref
    }
}

pub(super) fn validate_release_read<'a>(
    release_read: Option<&ReleaseReadConfig>,
    mut connection_refs: impl Iterator<Item = &'a KeychainReference>,
) -> Result<(), HostError> {
    let Some(release_read) = release_read else {
        return Ok(());
    };
    if connection_refs.any(|reference| reference == &release_read.credential_ref) {
        return Err(HostError::Config);
    }
    Ok(())
}
