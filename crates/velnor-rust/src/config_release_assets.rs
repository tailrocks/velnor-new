// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{PrivateSources, ReleaseAsset};

use crate::error::Reason;

const MAX_ASSETS: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawReleaseAsset {
    repo: String,
    tag: String,
    asset: String,
    sha256: String,
    dest: String,
}

pub(crate) fn parse(
    raw: Vec<RawReleaseAsset>,
    credentials: Option<&PrivateSources>,
) -> Result<Vec<ReleaseAsset>, Reason> {
    let invalid = |key: &'static str, value: &str| Reason::InvalidValue {
        key,
        value: value.to_owned(),
    };
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    if credentials.is_none() {
        return Err(invalid(
            "release_assets",
            "needs [private_sources] for the GitHub App credentials",
        ));
    }
    if raw.len() > MAX_ASSETS {
        return Err(invalid("release_assets", "more than 16 entries"));
    }
    let mut assets: Vec<ReleaseAsset> = Vec::new();
    for entry in &raw {
        let asset = ReleaseAsset::new(&entry.repo, &entry.tag, &entry.asset, &entry.sha256, &entry.dest)
            .map_err(|e| match e.field {
                "repo" => invalid("release_assets.repo", &e.value),
                "tag" => invalid("release_assets.tag", &e.value),
                "asset" => invalid("release_assets.asset", &e.value),
                "sha256" => invalid("release_assets.sha256", &e.value),
                _ => invalid("release_assets.dest", &e.value),
            })?;
        let clash = |other: &ReleaseAsset| {
            other.sha256 == asset.sha256 || (other.dest == asset.dest && other.name == asset.name)
        };
        if assets.first().is_some_and(|first| first.owner != asset.owner) {
            return Err(invalid("release_assets.repo", &entry.repo));
        }
        if assets.iter().any(clash) {
            return Err(invalid("release_assets.sha256", &entry.sha256));
        }
        assets.push(asset);
    }
    Ok(assets)
}
