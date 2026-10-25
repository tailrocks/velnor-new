use super::{HANDOFF_SCHEMA, PackageOutputManifest, invalid};
use crate::OrchestratorError;

pub(super) fn validate_expected_identity(
    source_commit: &str,
    source_ref: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    if source_commit.len() != 40
        || !source_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !source_ref.starts_with("refs/heads/")
        || source_ref.len() > 300
        || version.is_empty()
        || version.len() > 128
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err(invalid("trusted_package_identity_invalid"));
    }
    Ok(())
}

pub(super) fn validate_manifest_identity(
    manifest: &PackageOutputManifest,
    source_commit: &str,
    source_ref: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    if manifest.schema != HANDOFF_SCHEMA
        || manifest.source_commit != source_commit
        || manifest.source_ref != source_ref
        || manifest.version != version
    {
        return Err(invalid("handoff_source_or_version_mismatch"));
    }
    Ok(())
}
