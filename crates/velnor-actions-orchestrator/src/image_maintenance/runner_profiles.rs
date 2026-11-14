// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    OciImageConfiguration, has_image_revision, has_probe_profile,
};

use crate::{OrchestratorError, internal::internal};

pub(super) use super::runner_family_admission::ImageKind;
pub(super) use velnor_archive_guard::owned_archive_guard::oci_candidate::has_image_kind;

pub(super) fn validate_revision(
    configuration: &OciImageConfiguration,
    source_commit: &str,
) -> Result<(), OrchestratorError> {
    if has_image_revision(configuration, source_commit) {
        Ok(())
    } else {
        Err(internal("runner_image_revision"))
    }
}

pub(super) fn validate_probe_profile(
    configuration: &OciImageConfiguration,
) -> Result<(), OrchestratorError> {
    if has_probe_profile(configuration) {
        Ok(())
    } else {
        Err(internal("runner_image_probe_profile"))
    }
}
