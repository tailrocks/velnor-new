use super::{
    BTreeMap, BTreeSet, ContractError, HomebrewFormulaAssetLiteral, HomebrewFormulaReleaseLiterals,
    HomebrewFormulaSpec, PackageReleaseChannel, ValidatedHomebrewProjection,
    canonical_channel_version_matches, invalid, is_full_sha, is_sha256,
    release_version_matches_plan, safe_release_tag, safe_version, validate_projection_spec,
};

impl HomebrewFormulaReleaseLiterals {
    pub fn new(
        spec: &HomebrewFormulaSpec,
        projection: &ValidatedHomebrewProjection,
        source_sha: String,
        version: String,
        tag: String,
        mut asset_sha256: BTreeMap<String, String>,
    ) -> Result<Self, ContractError> {
        spec.validate()?;
        validate_projection_spec(spec, projection)?;
        if !is_full_sha(&source_sha)
            || source_sha == "0000000000000000000000000000000000000000"
            || !safe_version(&version)
            || !canonical_channel_version_matches(projection.channel, &source_sha, &version)
            || !release_version_matches_plan(
                projection.channel,
                &projection.base_version,
                &source_sha,
                &version,
            )
            || !safe_release_tag(&tag, projection.channel, &source_sha, &version)
        {
            return Err(invalid("malformed_release_literals"));
        }
        let expected = projection
            .selected
            .iter()
            .map(|payload| payload.asset_name.as_str())
            .collect::<BTreeSet<_>>();
        if asset_sha256.len() != expected.len()
            || asset_sha256
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                != expected
        {
            return Err(invalid("release_asset_inventory_mismatch"));
        }
        let assets = projection
            .selected
            .iter()
            .map(|payload| {
                let sha256 = asset_sha256
                    .remove(&payload.asset_name)
                    .ok_or_else(|| invalid("release_asset_digest_missing"))?;
                if !is_sha256(&sha256) {
                    return Err(invalid("release_asset_digest_malformed"));
                }
                if sha256.bytes().all(|byte| byte == b'0') {
                    return Err(invalid("release_asset_digest_zeroed"));
                }
                Ok(HomebrewFormulaAssetLiteral {
                    name: payload.asset_name.clone(),
                    url: format!(
                        "https://github.com/{}/releases/download/{}/{}",
                        projection.repository, tag, payload.asset_name
                    ),
                    sha256,
                })
            })
            .collect::<Result<Vec<_>, ContractError>>()?;
        Ok(Self {
            source_sha: Some(source_sha),
            version,
            tag: Some(tag),
            assets,
            channel: projection.channel,
            repository: projection.repository.clone(),
            selected: projection.selected.clone(),
            spec_digest: spec.digest()?,
            disabled_stable_sentinel: false,
        })
    }

    pub fn stable_sentinel(
        spec: &HomebrewFormulaSpec,
        projection: &ValidatedHomebrewProjection,
    ) -> Result<Self, ContractError> {
        spec.validate()?;
        if projection.channel != PackageReleaseChannel::Stable {
            return Err(invalid("stable_sentinel_requires_stable_projection"));
        }
        if projection.base_version == "0.0.0" {
            return Err(invalid("stable_version_collides_with_sentinel"));
        }
        validate_projection_spec(spec, projection)?;
        let source_sha = "0".repeat(40);
        let tag = "v0.0.0".to_owned();
        let assets = projection
            .selected
            .iter()
            .map(|payload| HomebrewFormulaAssetLiteral {
                name: payload.asset_name.clone(),
                url: format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    projection.repository, tag, payload.asset_name
                ),
                sha256: "0".repeat(64),
            })
            .collect();
        Ok(Self {
            source_sha: Some(source_sha),
            version: "0.0.0".to_owned(),
            tag: Some(tag),
            assets,
            channel: PackageReleaseChannel::Stable,
            repository: projection.repository.clone(),
            selected: projection.selected.clone(),
            spec_digest: spec.digest()?,
            disabled_stable_sentinel: true,
        })
    }

    #[must_use]
    pub fn source_sha(&self) -> Option<&str> {
        self.source_sha.as_deref()
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    #[must_use]
    pub fn assets(&self) -> &[HomebrewFormulaAssetLiteral] {
        &self.assets
    }

    #[must_use]
    pub const fn is_disabled_stable_sentinel(&self) -> bool {
        self.disabled_stable_sentinel
    }
}

impl HomebrewFormulaAssetLiteral {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}
