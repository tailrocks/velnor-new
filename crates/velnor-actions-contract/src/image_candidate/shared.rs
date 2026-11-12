// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ContractError;

pub use velnor_runner_image_manifest::{
    ExpectedBuildSource, ImageKind, RUNNER_ABSENCES_PATH, RUNNER_IMAGE_FAMILY_ARTIFACT_NAMES,
    RUNNER_IMAGE_PUBLICATION_SCHEMA, RUNNER_INVENTORY_PATH, RUNNER_QUALIFICATION_SCHEMA,
    RunnerImageNamedContext, RunnerImagePublicationLock, RunnerImagePublicationProvenance,
    RunnerImagePublicationRecord, RunnerImagePublicationSource, RunnerImageQualification,
    RunnerImageQualificationImage, RunnerImageQualificationImages, admits_build_source,
};

pub fn parse_runner_image_publication_record(
    bytes: &[u8],
) -> Result<RunnerImagePublicationRecord, ContractError> {
    velnor_runner_image_manifest::parse_runner_image_publication_record(bytes).map_err(Into::into)
}

pub fn validate_runner_image_publication_record(
    record: &RunnerImagePublicationRecord,
) -> Result<(), ContractError> {
    velnor_runner_image_manifest::validate_runner_image_publication_record(record)
        .map_err(Into::into)
}

pub fn parse_runner_image_qualification(
    bytes: &[u8],
) -> Result<RunnerImageQualification, ContractError> {
    velnor_runner_image_manifest::parse_runner_image_qualification(bytes).map_err(Into::into)
}

pub fn validate_runner_image_qualification(
    record: &RunnerImageQualification,
) -> Result<(), ContractError> {
    velnor_runner_image_manifest::validate_runner_image_qualification(record).map_err(Into::into)
}

pub fn runner_publication_source_inputs() -> Result<&'static [&'static str], ContractError> {
    velnor_runner_image_manifest::runner_publication_source_inputs().map_err(Into::into)
}

pub fn runner_publication_source_absences() -> Result<&'static [&'static str], ContractError> {
    velnor_runner_image_manifest::runner_publication_source_absences().map_err(Into::into)
}

pub fn parse_runner_publication_source_inputs(raw: &str) -> Result<Vec<&str>, ContractError> {
    velnor_runner_image_manifest::parse_runner_publication_source_inputs(raw).map_err(Into::into)
}

pub fn parse_runner_publication_source_absences<'a>(
    raw: &'a str,
    positive: &[&str],
) -> Result<Vec<&'a str>, ContractError> {
    velnor_runner_image_manifest::parse_runner_publication_source_absences(raw, positive)
        .map_err(Into::into)
}
