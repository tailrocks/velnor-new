use std::path::Path;

use serde::Serialize;

use crate::error::HostError;

const REPOSITORY: &str = "tailrocks/velnor-new";
const SOURCE_REF: &str = "refs/heads/main";
const SIGNER_WORKFLOW: &str = ".github/workflows/product-release-images.yml";
const CERTIFICATE_IDENTITY: &str = "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main";
const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const ARCHIVE_NAME: &str = "velnor-resource-probe-linux-amd64.tar";
const ARCHIVE_FORMAT: &str = "buildkit-oci-layout-docker-compat-v1";
const PLATFORM: &str = "linux/amd64";
const IMAGE_USER: &str = "65532:65532";
const ENTRYPOINT: &str = "/velnor/resource-probe";

#[derive(Serialize)]
struct ReleaseManifest<'a> {
    schema_version: u8,
    repository: &'static str,
    source_ref: &'static str,
    source_commit: &'a str,
    signer_workflow: &'static str,
    signer_ref: &'static str,
    workflow_authority_sha: &'a str,
    trusted_signing_identity: SigningIdentity,
    platform: &'static str,
    archive_format: &'static str,
    oci_index_sha256: &'a str,
    image_manifest_digest: &'a str,
    config_digest: &'a str,
    archive_name: &'static str,
    archive_sha256: String,
    protocol_version: u8,
    image_user: &'static str,
    entrypoint: &'static str,
}

#[derive(Serialize)]
struct SigningIdentity {
    oidc_issuer: &'static str,
    certificate_identity: &'static str,
}

struct ReleaseInputs {
    source_sha: String,
    authority_sha: String,
    inspect: Vec<u8>,
    archive: Vec<u8>,
}

pub(super) fn write(
    inspect_path: &Path,
    archive_path: &Path,
    output_path: &Path,
) -> Result<(), HostError> {
    let inputs = load_inputs(inspect_path, archive_path)?;
    let identity = crate::artifact_admission::inspect_release_archive(
        &inputs.inspect,
        &inputs.archive,
        &inputs.source_sha,
    )?;
    let manifest = manifest_bytes(&inputs, &identity)?;
    super::files::create_new_regular(output_path, &manifest)
}

fn load_inputs(inspect_path: &Path, archive_path: &Path) -> Result<ReleaseInputs, HostError> {
    let source_sha = std::env::var("VELNOR_SOURCE_SHA").map_err(|_| HostError::Identity)?;
    let authority_sha =
        std::env::var("VELNOR_WORKFLOW_AUTHORITY_SHA").map_err(|_| HostError::Identity)?;
    validate_workflow_identity(&source_sha, &authority_sha)?;
    Ok(ReleaseInputs {
        source_sha,
        authority_sha,
        inspect: read_bounded(
            inspect_path,
            crate::artifact_admission::RELEASE_MANIFEST_LIMIT,
        )?,
        archive: read_bounded(
            archive_path,
            crate::artifact_admission::RELEASE_ARCHIVE_LIMIT,
        )?,
    })
}

fn validate_workflow_identity(source: &str, authority: &str) -> Result<(), HostError> {
    if !is_lower_hex(source, 40)
        || !is_lower_hex(authority, 40)
        || source != authority
        || std::env::var("GITHUB_REPOSITORY").as_deref() != Ok(REPOSITORY)
        || std::env::var("GITHUB_REF").as_deref() != Ok(SOURCE_REF)
        || std::env::var("GITHUB_SHA").as_deref() != Ok(source)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, HostError> {
    super::files::read_bounded_regular(path, limit)
}

fn manifest_bytes(
    inputs: &ReleaseInputs,
    identity: &crate::artifact_admission::ReleaseArchiveIdentity,
) -> Result<Vec<u8>, HostError> {
    let manifest = ReleaseManifest {
        schema_version: 1,
        repository: REPOSITORY,
        source_ref: SOURCE_REF,
        source_commit: &inputs.source_sha,
        signer_workflow: SIGNER_WORKFLOW,
        signer_ref: SOURCE_REF,
        workflow_authority_sha: &inputs.authority_sha,
        trusted_signing_identity: SigningIdentity {
            oidc_issuer: OIDC_ISSUER,
            certificate_identity: CERTIFICATE_IDENTITY,
        },
        platform: PLATFORM,
        archive_format: ARCHIVE_FORMAT,
        oci_index_sha256: &identity.oci_index_sha256,
        image_manifest_digest: &identity.image_manifest_digest,
        config_digest: &identity.config_digest,
        archive_name: ARCHIVE_NAME,
        archive_sha256: crate::artifact_admission::hash_release_bytes(&inputs.archive),
        protocol_version: 1,
        image_user: IMAGE_USER,
        entrypoint: ENTRYPOINT,
    };
    let mut bytes = serde_json::to_vec(&manifest).map_err(|_| HostError::Identity)?;
    bytes.push(b'\n');
    if bytes.len() > 64 * 1024 {
        return Err(HostError::Frame);
    }
    Ok(bytes)
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::{ReleaseInputs, manifest_bytes};
    use crate::artifact_admission::ReleaseArchiveIdentity;
    use crate::error::HostError;

    #[test]
    fn emits_the_fixed_release_schema_from_verified_identity() -> Result<(), HostError> {
        let inputs = ReleaseInputs {
            source_sha: "a".repeat(40),
            authority_sha: "a".repeat(40),
            inspect: Vec::new(),
            archive: b"validated-archive".to_vec(),
        };
        let identity = ReleaseArchiveIdentity {
            oci_index_sha256: "b".repeat(64),
            image_manifest_digest: format!("sha256:{}", "c".repeat(64)),
            config_digest: format!("sha256:{}", "d".repeat(64)),
        };
        let bytes = manifest_bytes(&inputs, &identity)?;
        assert!(bytes.ends_with(b"\n"));
        assert!(!bytes[..bytes.len() - 1].contains(&b'\n'));
        let value = crate::artifact_admission::parse_release_json(&bytes[..bytes.len() - 1])?;
        assert_eq!(
            value
                .get("source_commit")
                .and_then(serde_json::Value::as_str),
            Some("a".repeat(40).as_str())
        );
        assert_eq!(
            value
                .get("oci_index_sha256")
                .and_then(serde_json::Value::as_str),
            Some("b".repeat(64).as_str())
        );
        assert_eq!(
            value
                .get("image_manifest_digest")
                .and_then(serde_json::Value::as_str),
            Some(format!("sha256:{}", "c".repeat(64)).as_str())
        );
        assert_eq!(
            value
                .get("config_digest")
                .and_then(serde_json::Value::as_str),
            Some(format!("sha256:{}", "d".repeat(64)).as_str())
        );
        assert_eq!(value.as_object().map(serde_json::Map::len), Some(18));
        Ok(())
    }
}
