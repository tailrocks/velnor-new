use super::types::{
    QualificationRuntimeIdentityRequirements, QualificationRuntimeImageDigests,
    QualificationRuntimeImageIdentity, QualificationRuntimePlatform,
};
use crate::errors::ContractError;

pub(super) fn validate_image_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
    platform: &QualificationRuntimePlatform,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;
    use crate::config::RunsOn;

    match (RunsOn::parse(&platform.runs_on)?, &platform.image_identity) {
        (
            RunsOn::Hosted(label),
            QualificationRuntimeImageIdentity::Hosted {
                image_os,
                image_version,
            },
        ) if label == platform.runner_label => {
            for (field, value) in [
                ("image_os", image_os.as_str()),
                ("image_version", image_version.as_str()),
            ] {
                validate_semantic_text(field, value)?;
                if value == crate::UNOBSERVED_IMAGE_VALUE {
                    return Err(ContractError::identity(
                        "qualification.runtime_identity",
                        format!("unobserved:{field}"),
                    ));
                }
            }
        }
        (
            RunsOn::ScaleSet(selector),
            QualificationRuntimeImageIdentity::ScaleSet {
                source_sha,
                guest_platform,
                scale_set,
                runner_image,
                dind_image,
            },
        ) => {
            if selector.token() != platform.runs_on
                || selector.name() != scale_set.name
                || selector.labels() != scale_set.labels
                || guest_platform != "linux/amd64"
                || platform.os != "Linux"
                || platform.arch != "X64"
                || platform.target != "x86_64-unknown-linux-gnu"
                || source_sha.len() != 40
                || !source_sha
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                || requirements.scale_set_image_source_sha.as_deref() != Some(source_sha)
            {
                return Err(ContractError::identity(
                    "qualification.runtime_identity.image_identity",
                    if requirements.scale_set_image_source_sha.is_none() {
                        "missing_expected_scale_set_image_source"
                    } else {
                        "scale_set_image_mismatch"
                    },
                ));
            }
            for label in &scale_set.labels {
                validate_semantic_text("scale_set_label", label)?;
            }
            validate_semantic_text("scale_set_name", &scale_set.name)?;
            validate_image_digests(runner_image)?;
            validate_image_digests(dind_image)?;
        }
        _ => {
            return Err(ContractError::identity(
                "qualification.runtime_identity.image_identity",
                "image_source_mismatch",
            ));
        }
    }
    Ok(())
}

fn validate_image_digests(image: &QualificationRuntimeImageDigests) -> Result<(), ContractError> {
    let valid_hex = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    if !valid_hex(&image.archive_sha256)
        || !valid_hex(&image.archive_index_digest)
        || image
            .image_index_digest
            .as_deref()
            .is_some_and(|digest| !valid_oci_digest(digest))
        || !valid_oci_digest(&image.manifest_digest)
        || !valid_oci_digest(&image.config_digest)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.image_identity",
            "malformed_scale_set_image_digest",
        ));
    }
    Ok(())
}

fn valid_oci_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
