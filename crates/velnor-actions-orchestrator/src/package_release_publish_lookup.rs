// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, JSON_LIMIT, ReleaseAssetResponse, ReleaseResponse, graphql_arguments};

impl GitHubApi {
    pub(in crate::package_release_publish) fn complete_release_assets(
        &self,
        release_id: u64,
    ) -> Result<Vec<ReleaseAssetResponse>, OrchestratorError> {
        if release_id == 0 {
            return Err(internal("published_release_id_invalid"));
        }
        let mut assets = Vec::new();
        for page in 1..=42 {
            let entries: Vec<CompleteAsset> = self.get_json(&self.route(&format!(
                "releases/{release_id}/assets?per_page=100&page={page}"
            )))?;
            let count = entries.len();
            if count > 100 || assets.len() + count > 4096 {
                return Err(internal("published_asset_inventory_bound"));
            }
            for asset in entries {
                if asset.id == 0
                    || asset.name.is_empty()
                    || asset.size == 0
                    || asset.state != "uploaded"
                {
                    return Err(internal("published_asset_state_invalid"));
                }
                assets.push(ReleaseAssetResponse {
                    id: asset.id,
                    name: asset.name,
                    size: asset.size,
                    digest: asset.digest,
                });
            }
            if count < 100 {
                let ids: BTreeSet<_> = assets.iter().map(|asset| asset.id).collect();
                let names: BTreeSet<_> = assets.iter().map(|asset| &asset.name).collect();
                if ids.len() != assets.len() || names.len() != assets.len() {
                    return Err(internal("published_asset_inventory_collision"));
                }
                return Ok(assets);
            }
        }
        Err(internal("published_asset_inventory_pages"))
    }

    pub(in crate::package_release_publish) fn release_by_tag(
        &self,
        tag: &str,
    ) -> Result<Option<ReleaseResponse>, OrchestratorError> {
        let (owner, name) = self
            .repository
            .split_once('/')
            .ok_or_else(|| internal("package_publish_repository_identity_invalid"))?;
        let response: serde_json::Value = self.graphql(
            "query($owner:String!,$name:String!,$tagName:String!){repository(owner:$owner,name:$name){release(tagName:$tagName){databaseId isDraft}}}",
            &[
                ("owner", owner.to_owned()),
                ("name", name.to_owned()),
                ("tagName", tag.to_owned()),
            ],
        )?;
        let repository = response
            .get("data")
            .and_then(|value| value.get("repository"))
            .ok_or_else(|| internal("package_publish_release_node_invalid"))?;
        let release = repository
            .get("release")
            .ok_or_else(|| internal("package_publish_release_node_invalid"))?;
        let Some(release) = (!release.is_null()).then_some(release) else {
            return Ok(None);
        };
        let id = release
            .get("databaseId")
            .and_then(serde_json::Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("package_publish_release_node_invalid"))?;
        let draft = release
            .get("isDraft")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| internal("package_publish_release_node_invalid"))?;
        let response: ReleaseResponse = self.get_json(&self.route(&format!("releases/{id}")))?;
        if response.id != id || response.tag_name != tag || response.draft != Some(draft) {
            return Err(internal("package_publish_release_tag_mismatch"));
        }
        Ok(Some(response))
    }

    fn graphql(
        &self,
        query: &str,
        fields: &[(&str, String)],
    ) -> Result<serde_json::Value, OrchestratorError> {
        let response = serde_json::from_slice(&self.graphql_bytes(query, fields)?)
            .map_err(|_| internal("package_publish_api_json"))?;
        Ok(response)
    }

    pub(in crate::package_release_publish) fn graphql_bytes(
        &self,
        query: &str,
        fields: &[(&str, String)],
    ) -> Result<Vec<u8>, OrchestratorError> {
        let bytes = self.request(graphql_arguments(query, fields), JSON_LIMIT)?;
        let response: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| internal("package_publish_api_json"))?;
        if response
            .get("errors")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(internal("package_publish_api_graphql_errors"));
        }
        Ok(bytes)
    }
}

#[derive(serde::Deserialize)]
struct CompleteAsset {
    id: u64,
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
}
