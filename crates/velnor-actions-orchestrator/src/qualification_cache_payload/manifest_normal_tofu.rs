use super::super::normal_tofu::ValidatedTofuRuntime;
use super::super::roots::CacheRootSet;
use super::parse::{schema, validate_digest};
use super::root::validate_payload_roots;
use super::{PayloadManifest, PayloadRoot, PayloadScope};
use serde_json::{Map, Value, json};

impl PayloadManifest {
    pub(crate) fn for_normal_tofu_save(
        cache_key: &str,
        runtime: &ValidatedTofuRuntime,
        state_digest: &str,
        roots: &CacheRootSet,
        payload_roots: Vec<PayloadRoot>,
    ) -> Result<Self, &'static str> {
        validate_digest(&runtime.runtime_identity_digest)?;
        validate_digest(state_digest)?;
        validate_payload_roots(roots, &payload_roots)?;
        if roots.layer.as_str() != "tofu_providers"
            || roots.owner_id != "normal-tofu-providers"
            || super::super::normal_tofu::cache_key(
                &runtime.target,
                &runtime.tofu_version,
                &runtime.root,
                &runtime.lockfile_sha256,
            )? != cache_key
        {
            return Err("normal_tofu_manifest_binding_mismatch");
        }
        Ok(Self {
            scope: PayloadScope::NormalTofuProviders {
                target: runtime.target.clone(),
                tofu_version: runtime.tofu_version.clone(),
                root: runtime.root.clone(),
                lockfile_sha256: runtime.lockfile_sha256.clone(),
            },
            cache_key: cache_key.to_owned(),
            runtime_identity_digest: runtime.runtime_identity_digest.clone(),
            state_digest: state_digest.to_owned(),
            roots: payload_roots,
        })
    }

    pub(crate) fn parse_for_normal_tofu_restore(
        text: &str,
        cache_key: &str,
        runtime: &ValidatedTofuRuntime,
        roots: &CacheRootSet,
    ) -> Result<Self, &'static str> {
        let manifest = super::parse::parse_canonical(text)?;
        validate_digest(&runtime.runtime_identity_digest)?;
        let expected_scope = PayloadScope::NormalTofuProviders {
            target: runtime.target.clone(),
            tofu_version: runtime.tofu_version.clone(),
            root: runtime.root.clone(),
            lockfile_sha256: runtime.lockfile_sha256.clone(),
        };
        if roots.owner_id != "normal-tofu-providers"
            || roots.layer.as_str() != "tofu_providers"
            || super::super::normal_tofu::cache_key(
                &runtime.target,
                &runtime.tofu_version,
                &runtime.root,
                &runtime.lockfile_sha256,
            )? != cache_key
            || manifest.scope != expected_scope
            || manifest.cache_key != cache_key
            || manifest.runtime_identity_digest != runtime.runtime_identity_digest
        {
            return Err("normal_tofu_manifest_binding_mismatch");
        }
        validate_payload_roots(roots, &manifest.roots)
            .map_err(|_| "normal_tofu_manifest_binding_mismatch")?;
        Ok(manifest)
    }
}

pub(super) fn to_json(manifest: &PayloadManifest, roots: &[Value]) -> Value {
    let PayloadScope::NormalTofuProviders {
        target,
        tofu_version,
        root,
        lockfile_sha256,
    } = &manifest.scope
    else {
        unreachable!("caller selects the normal ToFu scope")
    };
    json!({
        "cache_key": manifest.cache_key,
        "layer": "tofu_providers",
        "lockfile_sha256": lockfile_sha256,
        "namespace": "normal_tofu_providers",
        "root": root,
        "roots": roots,
        "runtime_identity_digest": manifest.runtime_identity_digest,
        "schema": schema(),
        "state_digest": manifest.state_digest,
        "target": target,
        "tofu_version": tofu_version,
    })
}

pub(super) fn parse_scope(object: &Map<String, Value>) -> Result<PayloadScope, &'static str> {
    if !has_exact_keys(
        object,
        &[
            "cache_key",
            "layer",
            "lockfile_sha256",
            "namespace",
            "root",
            "roots",
            "runtime_identity_digest",
            "schema",
            "state_digest",
            "target",
            "tofu_version",
        ],
    ) || string_field(object, "layer")? != "tofu_providers"
    {
        return Err("qualification_cache_manifest_invalid");
    }
    let lockfile_sha256 = string_field(object, "lockfile_sha256")?.to_owned();
    super::super::normal_tofu::validate_sha256(&lockfile_sha256)?;
    Ok(PayloadScope::NormalTofuProviders {
        target: string_field(object, "target")?.to_owned(),
        tofu_version: string_field(object, "tofu_version")?.to_owned(),
        root: object
            .get("root")
            .and_then(Value::as_str)
            .ok_or("qualification_cache_manifest_invalid")?
            .to_owned(),
        lockfile_sha256,
    })
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
#[path = "tests/normal_tofu_manifest_restore_tests.rs"]
mod tests;
