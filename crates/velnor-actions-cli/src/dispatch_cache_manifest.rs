//! Canonical manifest for one isolated cache payload.
//!
//! The manifest is deliberately independent of a workflow run ID so the
//! same admitted cache key can move from a cold producer to its warm consumer.

use serde_json::{Map, Value, json};
use velnor_actions_contract::{canonical_json_str, strict_json::parse_strict_json_with_limit};
use velnor_actions_orchestrator::QualificationCacheBindingOutputs;

use super::roots::{CacheRootId, CacheRootSet};

const SCHEMA: &str = "velnor.cache-payload/v2";
const NAMESPACE: &str = "qualification";
const MAX_MANIFEST_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PayloadRoot {
    pub(crate) id: CacheRootId,
    pub(crate) present: bool,
    pub(crate) digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PayloadManifest {
    pub(crate) campaign: String,
    pub(crate) matrix_key: String,
    pub(crate) layer: String,
    pub(crate) cache_key: String,
    pub(crate) runtime_identity_digest: String,
    pub(crate) state_digest: String,
    pub(crate) roots: Vec<PayloadRoot>,
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
            || binding.matrix_key != roots.matrix_key
            || binding.save_key.as_deref() != Some(cache_key)
        {
            return Err("qualification_cache_manifest_binding_mismatch");
        }
        Ok(Self {
            campaign: binding.campaign.clone(),
            matrix_key: binding.matrix_key.clone(),
            layer: binding.layer.clone(),
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
        let manifest = parse_value(&value)?;
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
            || binding.matrix_key != roots.matrix_key
            || binding.expected_cache.is_none()
            || binding.restore_key.as_deref() != Some(cache_key)
            || manifest.campaign != binding.campaign
            || manifest.matrix_key != binding.matrix_key
            || manifest.layer != binding.layer
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
        let manifest = parse_canonical(text)?;
        let runtime_identity_digest = binding
            .runtime_identity_digest
            .as_deref()
            .ok_or("qualification_cache_manifest_identity_missing")?;
        validate_digest(runtime_identity_digest)?;
        if !binding.active
            || binding.layer != roots.layer.as_str()
            || binding.matrix_key != roots.matrix_key
            || binding.save_key.as_deref() != Some(cache_key)
            || manifest.campaign != binding.campaign
            || manifest.matrix_key != binding.matrix_key
            || manifest.layer != binding.layer
            || manifest.cache_key != cache_key
            || manifest.runtime_identity_digest != runtime_identity_digest
        {
            return Err("qualification_cache_manifest_binding_mismatch");
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
                    "present": root.present,
                })
            })
            .collect::<Vec<_>>();
        let value = json!({
            "cache_key": self.cache_key,
            "campaign": self.campaign,
            "layer": self.layer,
            "matrix_key": self.matrix_key,
            "namespace": NAMESPACE,
            "roots": roots,
            "runtime_identity_digest": self.runtime_identity_digest,
            "schema": SCHEMA,
            "state_digest": self.state_digest,
        });
        let text =
            canonical_json_str(&value).map_err(|_| "qualification_cache_manifest_invalid")?;
        if text.len() > MAX_MANIFEST_BYTES {
            return Err("qualification_cache_manifest_limit");
        }
        Ok(text)
    }
}

fn parse_canonical(text: &str) -> Result<PayloadManifest, &'static str> {
    if text.len() > MAX_MANIFEST_BYTES || text.chars().any(|value| matches!(value, '\n' | '\r')) {
        return Err("qualification_cache_manifest_invalid");
    }
    let value = parse_strict_json_with_limit(text, MAX_MANIFEST_BYTES)
        .map_err(|_| "qualification_cache_manifest_invalid")?;
    let canonical =
        canonical_json_str(&value).map_err(|_| "qualification_cache_manifest_invalid")?;
    if canonical != text {
        return Err("qualification_cache_manifest_noncanonical");
    }
    parse_value(&value)
}

fn parse_value(value: &Value) -> Result<PayloadManifest, &'static str> {
    let object = value
        .as_object()
        .filter(|object| {
            has_exact_keys(
                object,
                &[
                    "cache_key",
                    "campaign",
                    "layer",
                    "matrix_key",
                    "namespace",
                    "roots",
                    "runtime_identity_digest",
                    "schema",
                    "state_digest",
                ],
            )
        })
        .ok_or("qualification_cache_manifest_invalid")?;
    if string_field(object, "schema")? != SCHEMA || string_field(object, "namespace")? != NAMESPACE
    {
        return Err("qualification_cache_manifest_schema_invalid");
    }
    let state_digest = string_field(object, "state_digest")?.to_owned();
    validate_digest(&state_digest)?;
    let roots_value = object
        .get("roots")
        .and_then(Value::as_array)
        .filter(|roots| !roots.is_empty() && roots.len() <= 8)
        .ok_or("qualification_cache_manifest_invalid")?;
    let mut roots = Vec::with_capacity(roots_value.len());
    for root in roots_value {
        roots.push(parse_root(root)?);
    }
    if roots
        .windows(2)
        .any(|pair| root_id_text(pair[0].id) >= root_id_text(pair[1].id))
    {
        return Err("qualification_cache_manifest_roots_order_invalid");
    }
    Ok(PayloadManifest {
        campaign: string_field(object, "campaign")?.to_owned(),
        matrix_key: string_field(object, "matrix_key")?.to_owned(),
        layer: string_field(object, "layer")?.to_owned(),
        cache_key: string_field(object, "cache_key")?.to_owned(),
        runtime_identity_digest: string_field(object, "runtime_identity_digest")?.to_owned(),
        state_digest,
        roots,
    })
}

fn parse_root(value: &Value) -> Result<PayloadRoot, &'static str> {
    let object = value
        .as_object()
        .filter(|object| has_exact_keys(object, &["digest", "id", "kind", "present"]))
        .ok_or("qualification_cache_manifest_invalid")?;
    let id =
        parse_root_id(string_field(object, "id")?).ok_or("qualification_cache_manifest_invalid")?;
    if string_field(object, "kind")? != root_kind(id) {
        return Err("qualification_cache_manifest_root_kind_invalid");
    }
    let present = object
        .get("present")
        .and_then(Value::as_bool)
        .ok_or("qualification_cache_manifest_invalid")?;
    let digest = match object.get("digest") {
        Some(Value::Null) if !present => None,
        Some(Value::String(value)) if present => {
            validate_digest(value)?;
            Some(value.clone())
        }
        _ => return Err("qualification_cache_manifest_invalid"),
    };
    Ok(PayloadRoot {
        id,
        present,
        digest,
    })
}

fn validate_payload_roots(
    roots: &CacheRootSet,
    payload_roots: &[PayloadRoot],
) -> Result<(), &'static str> {
    if roots.roots.len() != payload_roots.len()
        || roots
            .roots
            .iter()
            .zip(payload_roots)
            .any(|(observed, payload)| observed.id != payload.id)
        || payload_roots.iter().any(|root| {
            root.present != root.digest.is_some()
                || root
                    .digest
                    .as_deref()
                    .is_some_and(|digest| validate_digest(digest).is_err())
        })
    {
        return Err("qualification_cache_manifest_roots_mismatch");
    }
    Ok(())
}

fn parse_root_id(value: &str) -> Option<CacheRootId> {
    Some(match value {
        "mbx_actions" => CacheRootId::MbxActions,
        "cargo_git_db" => CacheRootId::CargoGitDb,
        "cargo_registry_cache" => CacheRootId::CargoRegistryCache,
        "cargo_registry_index" => CacheRootId::CargoRegistryIndex,
        "cargo_bin" => CacheRootId::CargoBin,
        "cargo_crates2_json" => CacheRootId::CargoCrates2Json,
        "cargo_crates_toml" => CacheRootId::CargoCratesToml,
        "mise_data" => CacheRootId::MiseData,
        "rustup_home" => CacheRootId::RustupHome,
        "tofu_plugins" => CacheRootId::TofuPlugins,
        "task_artifacts" => CacheRootId::TaskArtifacts,
        _ => return None,
    })
}

fn root_id_text(id: CacheRootId) -> &'static str {
    match id {
        CacheRootId::MbxActions => "mbx_actions",
        CacheRootId::CargoGitDb => "cargo_git_db",
        CacheRootId::CargoRegistryCache => "cargo_registry_cache",
        CacheRootId::CargoRegistryIndex => "cargo_registry_index",
        CacheRootId::CargoBin => "cargo_bin",
        CacheRootId::CargoCrates2Json => "cargo_crates2_json",
        CacheRootId::CargoCratesToml => "cargo_crates_toml",
        CacheRootId::MiseData => "mise_data",
        CacheRootId::RustupHome => "rustup_home",
        CacheRootId::TofuPlugins => "tofu_plugins",
        CacheRootId::TaskArtifacts => "task_artifacts",
    }
}

fn root_kind(id: CacheRootId) -> &'static str {
    match id {
        CacheRootId::CargoCrates2Json | CacheRootId::CargoCratesToml => "file",
        _ => "directory",
    }
}

fn validate_digest(value: &str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("b3-") else {
        return Err("qualification_cache_manifest_digest_invalid");
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("qualification_cache_manifest_digest_invalid");
    }
    Ok(())
}

fn has_exact_keys(object: &Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, &'static str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("qualification_cache_manifest_invalid")
}

#[cfg(test)]
#[path = "dispatch_cache_manifest_tests.rs"]
mod tests;
