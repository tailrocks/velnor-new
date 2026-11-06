// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

pub(crate) fn connection_pat(credential: &[u8]) -> Result<&str, HostError> {
    if credential.is_empty()
        || credential.len() > 4096
        || credential.iter().any(|byte| !byte.is_ascii_graphic())
    {
        return Err(HostError::Keychain);
    }
    std::str::from_utf8(credential).map_err(|_| HostError::Keychain)
}
