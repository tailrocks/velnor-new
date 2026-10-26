// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::HostError;

mod files;
mod manifest;
mod smoke;

pub fn validate_smoke(output: &Path, root: &Path) -> Result<(), HostError> {
    smoke::validate(output, root)
}

pub fn write_manifest(inspect: &Path, archive: &Path, output: &Path) -> Result<(), HostError> {
    manifest::write(inspect, archive, output)
}
