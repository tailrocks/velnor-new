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

#[cfg(all(test, unix))]
#[test]
fn attestation_temp_directory_is_private_at_creation() {
    use std::os::unix::fs::MetadataExt;

    let directory = private_attestation_dir().expect("private attestation directory");
    let mode = std::fs::metadata(directory.path())
        .expect("directory metadata")
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}
