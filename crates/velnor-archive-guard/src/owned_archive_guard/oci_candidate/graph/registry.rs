// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ConfigurationProjection, OciCandidateError, OciImageConfiguration, Result, check_deadline,
    parse, parse_manifest, project_configuration, support::sha256,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciRegistryManifest {
    manifest_digest: String,
    manifest_size: u64,
    config_digest: String,
    config_size: u64,
    layer_count: usize,
}

impl OciRegistryManifest {
    #[must_use]
    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }
    #[must_use]
    pub fn manifest_size(&self) -> u64 {
        self.manifest_size
    }
    #[must_use]
    pub fn config_digest(&self) -> &str {
        &self.config_digest
    }
    #[must_use]
    pub fn config_size(&self) -> u64 {
        self.config_size
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciRegistryProjection {
    pub manifest: OciRegistryManifest,
    pub configuration: OciImageConfiguration,
    pub diff_ids: Vec<String>,
}

pub fn inspect_registry_manifest(
    bytes: &[u8],
    expected_digest: &str,
) -> Result<OciRegistryManifest> {
    check_deadline()?;
    if bytes.is_empty()
        || bytes.len() > 4 * 1024 * 1024
        || format!("sha256:{}", sha256(bytes)) != expected_digest
    {
        return Err(OciCandidateError("oci_registry_manifest_digest"));
    }
    let (config, layers) = parse_manifest(bytes)?;
    check_deadline()?;
    Ok(OciRegistryManifest {
        manifest_digest: expected_digest.to_owned(),
        manifest_size: u64::try_from(bytes.len())
            .map_err(|_| OciCandidateError("oci_registry_manifest_size"))?,
        config_digest: format!("sha256:{}", config.digest),
        config_size: config.size,
        layer_count: layers.len(),
    })
}

pub fn inspect_registry_configuration(
    manifest: OciRegistryManifest,
    bytes: &[u8],
) -> Result<OciRegistryProjection> {
    check_deadline()?;
    if bytes.len() > 64 * 1024 * 1024
        || u64::try_from(bytes.len()).ok() != Some(manifest.config_size)
        || format!("sha256:{}", sha256(bytes)) != manifest.config_digest
    {
        return Err(OciCandidateError("oci_registry_config_digest"));
    }
    let config = parse(bytes)?;
    let (diff_ids, configuration) = project_configuration(
        &config,
        manifest.layer_count,
        ConfigurationProjection::Include,
    )?;
    if diff_ids.iter().any(|digest| {
        digest.strip_prefix("sha256:").is_none_or(|value| {
            value.len() != 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }) {
        return Err(OciCandidateError("oci_diff_id_shape"));
    }
    Ok(OciRegistryProjection {
        manifest,
        configuration,
        diff_ids,
    })
}
