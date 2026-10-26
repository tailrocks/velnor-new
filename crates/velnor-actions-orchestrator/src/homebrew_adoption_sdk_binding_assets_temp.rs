// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tempfile::TempDir;

use crate::OrchestratorError;

pub(super) fn private_attestation_dir() -> Result<TempDir, OrchestratorError> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("velnor-sdk-publication-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir()
        .map_err(|error| OrchestratorError::io("sdk_attestation_temp", error.to_string()))
}
