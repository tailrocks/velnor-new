//! Manifest with fixed fields and root order for one isolated cache payload.
//!
//! The manifest does not use a workflow run ID.
//! Cold and warm jobs use the same checked cache key.

use crate::qualification_cache_runtime::QualificationCacheBindingOutputs;
use serde_json::json;
use velnor_actions_contract::{canonical_json_str, strict_json::parse_strict_json_with_limit};

use super::roots::{CacheRootId, CacheRootSet};
#[path = "manifest_normal_key.rs"]
mod normal_key;
#[path = "manifest_roots.rs"]
mod root;
use normal_key::valid_normal_tools_key;
use root::{root_id_text, root_kind, valid_root_mode, validate_payload_roots};
#[path = "manifest_normal_tofu.rs"]
mod normal_tofu;
#[path = "manifest_parse.rs"]
mod parse;
pub(crate) use parse::{schema, validate_digest};

const NAMESPACE: &str = "qualification";
const NORMAL_NAMESPACE: &str = "normal_tools";
const MAX_MANIFEST_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PayloadRoot {
    pub(crate) id: CacheRootId,
    pub(crate) present: bool,
    pub(crate) mode: Option<u32>,
    pub(crate) digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PayloadManifest {
    pub(crate) scope: PayloadScope,
    pub(crate) cache_key: String,
    pub(crate) runtime_identity_digest: String,
    pub(crate) state_digest: String,
    pub(crate) roots: Vec<PayloadRoot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PayloadScope {
    Qualification {
        campaign: String,
        owner_id: String,
        layer: String,
    },
    NormalTools,
    NormalTofuProviders {
        target: String,
        tofu_version: String,
        root: String,
        lockfile_sha256: String,
    },
}

impl PayloadManifest {
    pub(crate) fn for_save(
        binding: &QualificationCacheBindingOutputs,
        roots: &CacheRootSet,
        cache_key: &str,
        state_digest: &str,
        payload_roots: Vec<PayloadRoot>,
    ) -> Result<Self, &'static str> {
        let runtime_identity_digest = binding
            .runtime_identity_digest
            .as_deref()
            .ok_or("qualification_cache_manifest_identity_missing")?;
        validate_digest(runtime_identity_digest)?;
        validate_digest(state_digest)?;
        validate_payload_roots(roots, &payload_roots)?;
        if !binding.active
            || binding.layer != roots.layer.as_str()
            || binding.owner_id != roots.owner_id
            || binding.save_key.as_deref() != Some(cache_key)
        {
            return Err("qualification_cache_manifest_binding_mismatch");
        }
        Ok(Self {
            scope: PayloadScope::Qualification {
                campaign: binding.campaign.clone(),
                owner_id: binding.owner_id.clone(),
                layer: binding.layer.clone(),
            },
            cache_key: cache_key.to_owned(),
            runtime_identity_digest: runtime_identity_digest.to_owned(),
            state_digest: state_digest.to_owned(),
            roots: payload_roots,
        })
    }

    pub(crate) fn parse_for_restore(
        text: &str,
        binding: &QualificationCacheBindingOutputs,
        roots: &CacheRootSet,
        cache_key: &str,
    ) -> Result<Self, &'static str> {
        if text.len() > MAX_MANIFEST_BYTES || text.chars().any(|value| matches!(value, '\n' | '\r'))
        {
            return Err("qualification_cache_manifest_invalid");
        }
        let value = parse_strict_json_with_limit(text, MAX_MANIFEST_BYTES)
            .map_err(|_| "qualification_cache_manifest_invalid")?;
        let canonical =
            canonical_json_str(&value).map_err(|_| "qualification_cache_manifest_invalid")?;
        if canonical != text {
            return Err("qualification_cache_manifest_noncanonical");
        }
        let manifest = parse::parse_value(&value)?;
        let runtime_identity_digest = binding
            .runtime_identity_digest
            .as_deref()
            .ok_or("qualification_cache_manifest_identity_missing")?;
        validate_digest(runtime_identity_digest)?;
        let expected_state_digest = binding
            .expected_prior_state_digest
            .as_deref()
            .ok_or("qualification_cache_manifest_state_missing")?;
        validate_digest(expected_state_digest)?;
        if !binding.active
            || binding.layer != roots.layer.as_str()
            || binding.owner_id != roots.owner_id
            || binding.expected_cache.is_none()
            || binding.restore_key.as_deref() != Some(cache_key)
            || manifest.scope
                != (PayloadScope::Qualification {
                    campaign: binding.campaign.clone(),
                    owner_id: binding.owner_id.clone(),
                    layer: binding.layer.clone(),
                })
            || manifest.cache_key != cache_key
            || manifest.runtime_identity_digest != runtime_identity_digest
            || manifest.state_digest != expected_state_digest
        {
            return Err("qualification_cache_manifest_binding_mismatch");
        }
        validate_payload_roots(roots, &manifest.roots)?;
        Ok(manifest)
    }

    pub(crate) fn parse_for_save(
        text: &str,
        binding: &QualificationCacheBindingOutputs,
        roots: &CacheRootSet,
        cache_key: &str,
    ) -> Result<Self, &'static str> {
        let manifest = parse::parse_canonical(text)?;
        let runtime_identity_digest = binding
            .runtime_identity_digest
            .as_deref()
            .ok_or("qualification_cache_manifest_identity_missing")?;
        validate_digest(runtime_identity_digest)?;
        if !binding.active
            || binding.layer != roots.layer.as_str()
            || binding.owner_id != roots.owner_id
            || binding.save_key.as_deref() != Some(cache_key)
            || manifest.scope
                != (PayloadScope::Qualification {
                    campaign: binding.campaign.clone(),
                    owner_id: binding.owner_id.clone(),
                    layer: binding.layer.clone(),
                })
            || manifest.cache_key != cache_key
            || manifest.runtime_identity_digest != runtime_identity_digest
        {
            return Err("qualification_cache_manifest_binding_mismatch");
        }
        validate_payload_roots(roots, &manifest.roots)?;
        Ok(manifest)
    }

    pub(crate) fn for_normal_save(
        cache_key: &str,
        runtime_identity_digest: &str,
        state_digest: &str,
        roots: &CacheRootSet,
        payload_roots: Vec<PayloadRoot>,
    ) -> Result<Self, &'static str> {
        validate_digest(runtime_identity_digest)?;
        validate_digest(state_digest)?;
        validate_payload_roots(roots, &payload_roots)?;
        if roots.layer.as_str() != "mise_tools" || !valid_normal_tools_key(cache_key) {
            return Err("normal_cache_manifest_binding_mismatch");
        }
        Ok(Self {
            scope: PayloadScope::NormalTools,
            cache_key: cache_key.to_owned(),
            runtime_identity_digest: runtime_identity_digest.to_owned(),
            state_digest: state_digest.to_owned(),
            roots: payload_roots,
        })
    }

    pub(crate) fn parse_for_normal_restore(
        text: &str,
        cache_key: &str,
        runtime_identity_digest: &str,
        roots: &CacheRootSet,
    ) -> Result<Self, &'static str> {
        let manifest = parse::parse_canonical(text)?;
        validate_digest(runtime_identity_digest)?;
        if roots.layer.as_str() != "mise_tools"
            || !valid_normal_tools_key(cache_key)
            || manifest.scope != PayloadScope::NormalTools
            || manifest.cache_key != cache_key
            || manifest.runtime_identity_digest != runtime_identity_digest
        {
            return Err("normal_cache_manifest_binding_mismatch");
        }
        validate_payload_roots(roots, &manifest.roots)?;
        Ok(manifest)
    }

    pub(crate) fn canonical_json(&self) -> Result<String, &'static str> {
        validate_digest(&self.state_digest)?;
        validate_digest(&self.runtime_identity_digest)?;
        if self
            .roots
            .windows(2)
            .any(|pair| root_id_text(pair[0].id) >= root_id_text(pair[1].id))
            || self.roots.iter().any(|root| {
                root.present != root.digest.is_some()
                    || root.present != root.mode.is_some()
                    || root.mode.is_some_and(|mode| !valid_root_mode(mode))
                    || root
                        .digest
                        .as_deref()
                        .is_some_and(|digest| validate_digest(digest).is_err())
            })
        {
            return Err("qualification_cache_manifest_roots_invalid");
        }
        let roots = self
            .roots
            .iter()
            .map(|root| {
                json!({
                    "digest": root.digest,
                    "id": root_id_text(root.id),
                    "kind": root_kind(root.id),
                    "mode": root.mode,
                    "present": root.present,
                })
            })
            .collect::<Vec<_>>();
        let value = match &self.scope {
            PayloadScope::Qualification {
                campaign,
                owner_id,
                layer,
            } => json!({
                "cache_key": self.cache_key,
                "campaign": campaign,
                "layer": layer,
                "owner_id": owner_id,
                "namespace": NAMESPACE,
                "roots": roots,
                "runtime_identity_digest": self.runtime_identity_digest,
                "schema": schema(),
                "state_digest": self.state_digest,
            }),
            PayloadScope::NormalTools => json!({
                "cache_key": self.cache_key,
                "layer": "mise_tools",
                "namespace": NORMAL_NAMESPACE,
                "roots": roots,
                "runtime_identity_digest": self.runtime_identity_digest,
                "schema": schema(),
                "state_digest": self.state_digest,
            }),
            PayloadScope::NormalTofuProviders { .. } => normal_tofu::to_json(self, &roots),
        };
        let text =
            canonical_json_str(&value).map_err(|_| "qualification_cache_manifest_invalid")?;
        if text.len() > MAX_MANIFEST_BYTES {
            return Err("qualification_cache_manifest_limit");
        }
        Ok(text)
    }
}

#[cfg(test)]
#[path = "tests/dispatch_cache_manifest_tests.rs"]
mod tests;
