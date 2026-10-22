//! Typed validation for Bun's verified private-fetch marker.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

use serde::Deserialize;
use velnor_actions_contract::config::is_valid_source_id;

use crate::OrchestratorError;
use crate::internal::internal;

use super::io_error;

const VERIFIED_FILE: &str = "verified.json";
const VERIFIED_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedDocument {
    schema: u32,
    sources: Vec<VerifiedSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedSource {
    kind: String,
    id: String,
    path: String,
    repository: String,
    tag: String,
    commit: String,
    asset: Option<String>,
    sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pin {
    pub(super) id: String,
    pub(super) sha256: String,
}

pub(super) fn parse_pins(raw: &str) -> Result<Vec<Pin>, OrchestratorError> {
    let mut pins = Vec::new();
    let mut ids = BTreeSet::new();
    for value in raw.split_whitespace() {
        let (id, digest) = value
            .split_once(':')
            .ok_or_else(|| internal("bun_assets_pin_invalid"))?;
        if !is_valid_source_id(id) || !is_lower_sha256(digest) || !ids.insert(id.to_owned()) {
            return Err(internal("bun_assets_pin_invalid"));
        }
        pins.push(Pin {
            id: id.to_owned(),
            sha256: digest.to_owned(),
        });
    }
    if pins.is_empty() {
        return Err(internal("bun_assets_pins_empty"));
    }
    pins.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(pins)
}

pub(super) fn verify_marker(deps: &Path, pins: &[Pin]) -> Result<(), OrchestratorError> {
    let path = deps.join(VERIFIED_FILE);
    let metadata = fs::symlink_metadata(&path).map_err(|err| io_error(&path, err))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > VERIFIED_LIMIT {
        return Err(internal("bun_assets_marker_unsafe"));
    }
    let document: VerifiedDocument =
        serde_json::from_slice(&fs::read(&path).map_err(|err| io_error(&path, err))?)
            .map_err(|_| internal("bun_assets_marker_invalid"))?;
    if document.schema != 1 || document.sources.is_empty() {
        return Err(internal("bun_assets_marker_invalid"));
    }
    let expected: BTreeMap<&str, &str> = pins
        .iter()
        .map(|pin| (pin.id.as_str(), pin.sha256.as_str()))
        .collect();
    let mut found = BTreeMap::new();
    for source in &document.sources {
        if source.kind != "release-assets"
            || !is_valid_source_id(&source.id)
            || source.path.is_empty()
            || source.repository.is_empty()
            || source.tag.is_empty()
            || source.commit.is_empty()
            || source.asset.as_deref().is_none_or(str::is_empty)
        {
            return Err(internal("bun_assets_marker_invalid"));
        }
        let Some(digest) = source
            .sha256
            .as_deref()
            .filter(|value| is_lower_sha256(value))
        else {
            return Err(internal("bun_assets_marker_invalid"));
        };
        validate_manifest_path(&source.path)?;
        let extracted = deps.join(&source.path);
        let metadata = fs::symlink_metadata(&extracted).map_err(|err| io_error(&extracted, err))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(internal("bun_assets_marker_path_invalid"));
        }
        if found.insert(source.id.as_str(), digest).is_some() {
            return Err(internal("bun_assets_marker_duplicate_id"));
        }
    }
    for pin in pins {
        if found.get(pin.id.as_str()).copied() != Some(pin.sha256.as_str()) {
            return Err(internal("bun_assets_pin_missing"));
        }
    }
    for (id, digest) in found {
        if expected.get(id).is_some_and(|expected| expected != &digest) {
            return Err(internal("bun_assets_pin_mismatch"));
        }
    }
    Ok(())
}

fn validate_manifest_path(value: &str) -> Result<(), OrchestratorError> {
    let path = Path::new(value);
    if value.contains('\\')
        || value.len() > 4096
        || value.as_bytes().get(1) == Some(&b':')
        || path.is_absolute()
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || value.split('/').count() > 256
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(internal("bun_assets_marker_path_invalid"));
    }
    Ok(())
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
