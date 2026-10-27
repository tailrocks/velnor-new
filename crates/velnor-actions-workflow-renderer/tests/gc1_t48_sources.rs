// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const IDENTITY_SOURCE: &str = include_str!("../src/job_image_authority_identity.py");
pub(super) const REGISTRY_SOURCE: &str = include_str!("../src/job_image_authority_registry.py");
pub(super) const PUBLISH_SOURCE: &str = include_str!("../src/job_image_authority_publish.py");
pub(super) const RECORD_SOURCE: &str = include_str!("../src/job_image_authority_record.py");
pub(super) const OCI_SOURCE: &str = include_str!("../src/job_image_authority_oci.py");
pub(super) const TAR_SOURCE: &str = include_str!("../src/job_image_authority_tar.py");

pub(super) const IDENTITY_FILE: &str = "job_image_authority_identity.py";
pub(super) const REGISTRY_FILE: &str = "job_image_authority_registry.py";
pub(super) const PUBLISH_FILE: &str = "job_image_authority_publish.py";
pub(super) const RECORD_FILE: &str = "job_image_authority_record.py";
pub(super) const OCI_FILE: &str = "job_image_authority_oci.py";
pub(super) const TAR_FILE: &str = "job_image_authority_tar.py";

pub(super) const STEP_NAME: &str = "Materialize fixed image authority implementation";
pub(super) const VERIFY_STEP_NAME: &str = "Verify candidate bytes with the fixed authority body";
pub(super) const EVIDENCE_STEP_NAME: &str =
    "Assemble publication evidence with the fixed authority body";
pub(super) const BODY_DIRECTORY: &str = "$RUNNER_TEMP/velnor-image-authority";
pub(super) const END_MARKER: &str = "VELNOR_IMAGE_AUTHORITY_EOF";
pub(super) const VERIFY_MARKER: &str = "VELNOR_IMAGE_VERIFY_EOF";
pub(super) const EVIDENCE_MARKER: &str = "VELNOR_IMAGE_EVIDENCE_EOF";
pub(super) const CANDIDATE_PATH: &str = ".github/workflows/job-image-candidate.yml";
