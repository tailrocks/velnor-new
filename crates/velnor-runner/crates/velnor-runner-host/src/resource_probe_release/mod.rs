//! Native validation and manifest production for the resource-probe OCI image.

use std::path::Path;

use crate::error::HostError;

mod files;
mod manifest;
mod schema5;
mod smoke;

/// Validate the one-record response produced by the restricted smoke run.
///
/// # Errors
///
/// Returns an error if the output is not a bounded canonical record or `root`
/// is not a directory.
pub fn validate_smoke(output: &Path, root: &Path) -> Result<(), HostError> {
    smoke::validate(output, root)
}

/// Validate one canonical schema 5 record from the restricted smoke run.
///
/// # Errors
///
/// Returns an error if the output is not a bounded canonical schema 5 record
/// or `root` is not a directory.
pub fn validate_smoke_schema5(output: &Path, root: &Path) -> Result<(), HostError> {
    schema5::validate(output, root)
}

/// Derive the release manifest from Docker's inspected image and saved archive.
///
/// # Errors
///
/// Returns an error if the source identity, inspected image, saved archive, or
/// output path fails release validation.
pub fn write_manifest(inspect: &Path, archive: &Path, output: &Path) -> Result<(), HostError> {
    manifest::write(inspect, archive, output)
}
