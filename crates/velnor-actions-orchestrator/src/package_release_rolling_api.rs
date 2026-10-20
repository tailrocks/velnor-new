//! Fixed ID-addressed rolling release transport and closed upload-state parsing.

use super::{GitHubApi, ReleaseAssetResponse, sha256};
use crate::{OrchestratorError, internal::internal};
use serde::Deserialize;
use std::collections::BTreeSet;
use velnor_actions_contract::publication_retry::rolling::{RollingAsset, RollingAssetState};

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    state: RollingAssetState,
    size: u64,
    digest: Option<String>,
}

impl GitHubApi {
    pub(in crate::package_release_publish) fn rolling_assets(
        &self,
        release_id: u64,
    ) -> Result<Vec<RollingAsset>, OrchestratorError> {
        let mut assets = Vec::new();
        for page in 1..=42 {
            let entries: Vec<Asset> = self.get_json(&self.route(&format!(
                "releases/{release_id}/assets?per_page=100&page={page}"
            )))?;
            let count = entries.len();
            if count > 100 || assets.len() + count > 4096 {
                return Err(internal("rolling_asset_inventory_bound"));
            }
            for asset in entries {
                assets.push(self.rolling_asset(asset)?);
            }
            if count < 100 {
                let ids: BTreeSet<_> = assets.iter().map(|asset| asset.id).collect();
                let names: BTreeSet<_> = assets.iter().map(|asset| &asset.name).collect();
                if ids.len() != assets.len() || names.len() != assets.len() {
                    return Err(internal("rolling_asset_inventory_collision"));
                }
                return Ok(assets);
            }
        }
        Err(internal("rolling_asset_inventory_pages"))
    }

    fn rolling_asset(&self, asset: Asset) -> Result<RollingAsset, OrchestratorError> {
        if asset.id == 0 || asset.name.is_empty() {
            return Err(internal("rolling_asset_identity"));
        }
        let digest = match asset.state {
            RollingAssetState::Starter if asset.size == 0 && asset.digest.is_none() => None,
            RollingAssetState::Uploaded if asset.size > 0 => {
                Some(if let Some(digest) = asset.digest {
                    digest
                        .strip_prefix("sha256:")
                        .filter(|value| {
                            value.len() == 64
                                && value
                                    .bytes()
                                    .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
                        })
                        .ok_or_else(|| internal("rolling_asset_digest"))?
                        .to_owned()
                } else {
                    let bytes = self.download_asset(asset.id)?;
                    if bytes.len() as u64 != asset.size {
                        return Err(internal("rolling_asset_size"));
                    }
                    sha256(&bytes)
                })
            }
            _ => return Err(internal("rolling_asset_state")),
        };
        Ok(RollingAsset {
            id: asset.id,
            name: asset.name,
            state: asset.state,
            size: asset.size,
            sha256: digest,
        })
    }

    pub(in crate::package_release_publish) fn complete_release_assets(
        &self,
        release_id: u64,
    ) -> Result<Vec<ReleaseAssetResponse>, OrchestratorError> {
        self.rolling_assets(release_id)?
            .into_iter()
            .map(|asset| {
                if asset.state != RollingAssetState::Uploaded {
                    return Err(internal("published_release_unfinished_asset"));
                }
                Ok(ReleaseAssetResponse {
                    id: asset.id,
                    name: asset.name,
                    size: asset.size,
                    digest: asset.sha256.map(|digest| format!("sha256:{digest}")),
                })
            })
            .collect()
    }

    pub(in crate::package_release_publish) fn delete_rolling_asset(
        &self,
        id: u64,
    ) -> Result<(), OrchestratorError> {
        if id == 0 {
            return Err(internal("rolling_asset_id"));
        }
        self.fields_request(&self.route(&format!("releases/assets/{id}")), "DELETE", &[])?;
        Ok(())
    }
}
