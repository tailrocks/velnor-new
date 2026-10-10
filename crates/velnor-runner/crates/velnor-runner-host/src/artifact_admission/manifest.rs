//! Exact schema for the source-bound resource-probe image release.

use serde::Deserialize;

use crate::compile_identity::CompiledReleaseIdentity;
use crate::error::HostError;

use super::hash::{is_lower_hex, is_sha256_digest};
use super::strict_json;

pub(super) const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub(super) const MANIFEST_ASSET: &str = "RESOURCE_PROBE_MANIFEST.json";
pub(super) const ARCHIVE_ASSET: &str = "velnor-resource-probe-linux-amd64.tar";
pub(super) const CHECKSUM_ASSET: &str = "SHA256SUMS";
pub(super) const IMAGE_TAG: &str = "velnor-resource-probe:linux-amd64";
pub(super) const PLATFORM: &str = "linux/amd64";
pub(super) const ARCHIVE_FORMAT: &str = "buildkit-oci-layout-docker-compat-v1";
pub(super) const IMAGE_USER: &str = "65532:65532";
pub(super) const IMAGE_ENTRYPOINT: &str = "/velnor/resource-probe";
pub(super) const IMAGE_WORKDIR: &str = "/";
pub(super) const SOURCE_LABEL: &str = "org.opencontainers.image.revision";
pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
pub(super) const SOURCE_REF: &str = "refs/heads/main";
pub(super) const SIGNER_WORKFLOW: &str = ".github/workflows/product-release-images.yml";
pub(super) const SIGNER_REF: &str = "refs/heads/main";
pub(super) const CERTIFICATE_IDENTITY: &str = "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main";
pub(super) const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
pub(super) const DEFAULT_ENV: &str =
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema_version: u8,
    repository: String,
    source_ref: String,
    source_commit: String,
    signer_workflow: String,
    signer_ref: String,
    workflow_authority_sha: String,
    trusted_signing_identity: SigningIdentity,
    platform: String,
    archive_format: String,
    oci_index_sha256: String,
    image_manifest_digest: String,
    config_digest: String,
    archive_name: String,
    archive_sha256: String,
    protocol_version: u8,
    image_user: String,
    entrypoint: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SigningIdentity {
    oidc_issuer: String,
    certificate_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProbeManifest {
    pub(super) source_commit: String,
    pub(super) workflow_authority_sha: String,
    pub(super) platform: String,
    pub(super) archive_format: String,
    pub(super) oci_index_sha256: String,
    pub(super) image_manifest_digest: String,
    pub(super) config_digest: String,
    pub(super) archive_sha256: String,
}

pub(super) fn parse(
    bytes: &[u8],
    identity: &CompiledReleaseIdentity,
) -> Result<ProbeManifest, HostError> {
    let source = super::hash::lowercase_hex(identity.source_sha());
    parse_for_source(bytes, &source)
}

fn parse_for_source(bytes: &[u8], source: &str) -> Result<ProbeManifest, HostError> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        return Err(HostError::Frame);
    }
    let strict = strict_json::parse(bytes)?;
    let value: RawManifest = serde_json::from_value(strict).map_err(|_| HostError::Identity)?;
    validate(&value, source)?;
    Ok(ProbeManifest {
        source_commit: value.source_commit,
        workflow_authority_sha: value.workflow_authority_sha,
        platform: value.platform,
        archive_format: value.archive_format,
        oci_index_sha256: value.oci_index_sha256,
        image_manifest_digest: value.image_manifest_digest,
        config_digest: value.config_digest,
        archive_sha256: value.archive_sha256,
    })
}

fn validate(manifest: &RawManifest, source: &str) -> Result<(), HostError> {
    if manifest.schema_version != 1
        || manifest.protocol_version != 1
        || manifest.repository != REPOSITORY
        || manifest.source_ref != SOURCE_REF
        || manifest.source_commit != source
        || manifest.signer_workflow != SIGNER_WORKFLOW
        || manifest.signer_ref != SIGNER_REF
        || manifest.workflow_authority_sha != source
        || manifest.trusted_signing_identity.oidc_issuer != OIDC_ISSUER
        || manifest.trusted_signing_identity.certificate_identity != CERTIFICATE_IDENTITY
        || manifest.platform != PLATFORM
        || manifest.archive_format != ARCHIVE_FORMAT
        || manifest.archive_name != ARCHIVE_ASSET
        || manifest.image_user != IMAGE_USER
        || manifest.entrypoint != IMAGE_ENTRYPOINT
        || !is_lower_hex(source, 40)
        || !is_lower_hex(&manifest.source_commit, 40)
        || !is_lower_hex(&manifest.workflow_authority_sha, 40)
        || !is_lower_hex(&manifest.oci_index_sha256, 64)
        || !is_sha256_digest(&manifest.image_manifest_digest)
        || !is_sha256_digest(&manifest.config_digest)
        || !is_lower_hex(&manifest.archive_sha256, 64)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ARCHIVE_ASSET, ARCHIVE_FORMAT, CERTIFICATE_IDENTITY, IMAGE_ENTRYPOINT, IMAGE_USER,
        OIDC_ISSUER, PLATFORM, REPOSITORY, SIGNER_REF, SIGNER_WORKFLOW, SOURCE_REF,
        parse_for_source,
    };
    use crate::error::HostError;

    fn manifest() -> String {
        format!(
            concat!(
                "{{\"schema_version\":1,\"repository\":\"{}\",\"source_ref\":\"{}\",",
                "\"source_commit\":\"{}\",\"signer_workflow\":\"{}\",",
                "\"signer_ref\":\"{}\",\"workflow_authority_sha\":\"{}\",",
                "\"trusted_signing_identity\":{{\"oidc_issuer\":\"{}\",",
                "\"certificate_identity\":\"{}\"}},\"platform\":\"{}\",",
                "\"archive_format\":\"{}\",\"oci_index_sha256\":\"{}\",",
                "\"image_manifest_digest\":\"sha256:{}\",\"config_digest\":\"sha256:{}\",",
                "\"archive_name\":\"{}\",\"archive_sha256\":\"{}\",",
                "\"protocol_version\":1,\"image_user\":\"{}\",\"entrypoint\":\"{}\"}}"
            ),
            REPOSITORY,
            SOURCE_REF,
            "11".repeat(20),
            SIGNER_WORKFLOW,
            SIGNER_REF,
            "11".repeat(20),
            OIDC_ISSUER,
            CERTIFICATE_IDENTITY,
            PLATFORM,
            ARCHIVE_FORMAT,
            "33".repeat(32),
            "44".repeat(32),
            "55".repeat(32),
            ARCHIVE_ASSET,
            "66".repeat(32),
            IMAGE_USER,
            IMAGE_ENTRYPOINT,
        )
    }

    #[test]
    fn accepts_exact_compiled_identity() -> Result<(), HostError> {
        let bytes = manifest();
        let parsed = parse_for_source(bytes.as_bytes(), &"11".repeat(20))?;
        if parsed.source_commit != "11".repeat(20) {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    #[test]
    fn rejects_identity_drift_unknown_fields_and_duplicate_fields() {
        let base = manifest();
        let wrong_source = base.replace(&"11".repeat(20), &"aa".repeat(20));
        assert_eq!(
            parse_for_source(wrong_source.as_bytes(), &"11".repeat(20)),
            Err(HostError::Identity)
        );
        let unknown = base.replace(
            "\"schema_version\":1,",
            "\"schema_version\":1,\"extra\":true,",
        );
        assert_eq!(
            parse_for_source(unknown.as_bytes(), &"11".repeat(20)),
            Err(HostError::Identity)
        );
        let duplicate = base.replacen(
            "\"schema_version\":1,",
            "\"schema_version\":1,\"schema_version\":1,",
            1,
        );
        assert_eq!(
            parse_for_source(duplicate.as_bytes(), &"11".repeat(20)),
            Err(HostError::Identity)
        );
    }
}
