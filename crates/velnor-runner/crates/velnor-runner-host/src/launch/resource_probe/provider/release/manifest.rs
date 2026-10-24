//! Strict source-bound release manifest parsing.

use serde_json::Value;

use super::super::{
    OIDC_ISSUER, PLATFORM, ProviderFailure, SIGNER_IDENTITY, SIGNER_WORKFLOW, SOURCE_REF,
    SOURCE_REPOSITORY,
};
use super::metadata::{valid_digest_field, valid_hex_field};
use super::{
    PROBE_ARCHIVE,
    metadata::{number, string},
};

#[derive(Debug)]
pub(in crate::launch::resource_probe::provider) struct ResourceManifest {
    pub(in crate::launch::resource_probe::provider) archive_sha256: String,
    pub(in crate::launch::resource_probe::provider) archive_name: String,
    pub(in crate::launch::resource_probe::provider) oci_index_sha256: String,
    pub(in crate::launch::resource_probe::provider) image_manifest_digest: String,
    pub(in crate::launch::resource_probe::provider) config_digest: String,
}

pub(super) fn parse_manifest(
    bytes: &[u8],
    source_revision: &str,
) -> Result<ResourceManifest, ProviderFailure> {
    const FIELDS: [&str; 18] = [
        "schema_version",
        "repository",
        "source_ref",
        "source_commit",
        "signer_workflow",
        "signer_ref",
        "workflow_authority_sha",
        "trusted_signing_identity",
        "platform",
        "archive_format",
        "oci_index_sha256",
        "image_manifest_digest",
        "config_digest",
        "archive_name",
        "archive_sha256",
        "protocol_version",
        "image_user",
        "entrypoint",
    ];
    let identity_fields = ["oidc_issuer", "certificate_identity"];
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Manifest)?;
    let object = value.as_object().ok_or(ProviderFailure::Manifest)?;
    let trusted_identity = object.get("trusted_signing_identity");
    if object.keys().any(|field| !FIELDS.contains(&field.as_str()))
        || object.len() != FIELDS.len()
        || trusted_identity
            .and_then(Value::as_object)
            .is_none_or(|identity| {
                identity.len() != identity_fields.len()
                    || identity
                        .keys()
                        .any(|field| !identity_fields.contains(&field.as_str()))
            })
        || number(object.get("schema_version")) != Some(1)
        || string(object.get("repository")) != Some(SOURCE_REPOSITORY)
        || string(object.get("source_ref")) != Some(SOURCE_REF)
        || string(object.get("source_commit")) != Some(source_revision)
        || string(object.get("signer_workflow")) != Some(SIGNER_WORKFLOW)
        || string(object.get("signer_ref")) != Some("refs/heads/main")
        || string(object.get("workflow_authority_sha")) != Some(source_revision)
        || string(object.get("platform")) != Some(PLATFORM)
        || string(object.get("archive_format")) != Some("buildkit-oci-layout-docker-compat-v1")
        || string(object.get("archive_name")) != Some(PROBE_ARCHIVE)
        || string(object.get("image_user")) != Some("65532:65532")
        || string(object.get("entrypoint")) != Some("/velnor/resource-probe")
        || number(object.get("protocol_version")) != Some(1)
        || trusted_identity.and_then(|identity| string(identity.get("oidc_issuer")))
            != Some(OIDC_ISSUER)
        || trusted_identity.and_then(|identity| string(identity.get("certificate_identity")))
            != Some(SIGNER_IDENTITY)
        || !valid_hex_field(object.get("archive_sha256"))
        || !valid_hex_field(object.get("oci_index_sha256"))
        || !valid_digest_field(object.get("image_manifest_digest"))
        || !valid_digest_field(object.get("config_digest"))
    {
        return Err(ProviderFailure::Manifest);
    }
    Ok(ResourceManifest {
        archive_sha256: string(object.get("archive_sha256"))
            .ok_or(ProviderFailure::Manifest)?
            .to_owned(),
        archive_name: string(object.get("archive_name"))
            .ok_or(ProviderFailure::Manifest)?
            .to_owned(),
        oci_index_sha256: string(object.get("oci_index_sha256"))
            .ok_or(ProviderFailure::Manifest)?
            .to_owned(),
        image_manifest_digest: string(object.get("image_manifest_digest"))
            .ok_or(ProviderFailure::Manifest)?
            .to_owned(),
        config_digest: string(object.get("config_digest"))
            .ok_or(ProviderFailure::Manifest)?
            .to_owned(),
    })
}
