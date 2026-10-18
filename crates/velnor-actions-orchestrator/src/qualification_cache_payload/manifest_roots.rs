use super::super::roots::{CacheRootId, CacheRootSet};
use super::PayloadRoot;
use serde_json::{Map, Value};

pub(super) fn parse_root(value: &Value) -> Result<PayloadRoot, &'static str> {
    let object = value
        .as_object()
        .filter(|object| has_exact_keys(object, &["digest", "id", "kind", "mode", "present"]))
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
            super::validate_digest(value)?;
            Some(value.clone())
        }
        _ => return Err("qualification_cache_manifest_invalid"),
    };
    let mode = match object.get("mode") {
        Some(Value::Null) if !present => None,
        Some(Value::Number(value)) if present => {
            let value = value
                .as_u64()
                .and_then(|mode| u32::try_from(mode).ok())
                .filter(|mode| valid_root_mode(*mode))
                .ok_or("qualification_cache_manifest_root_mode_invalid")?;
            Some(value)
        }
        _ => return Err("qualification_cache_manifest_invalid"),
    };
    Ok(PayloadRoot {
        id,
        present,
        mode,
        digest,
    })
}

pub(super) fn validate_payload_roots(
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
                || root.present != root.mode.is_some()
                || root.mode.is_some_and(|mode| !valid_root_mode(mode))
                || root
                    .digest
                    .as_deref()
                    .is_some_and(|digest| super::validate_digest(digest).is_err())
        })
    {
        return Err("qualification_cache_manifest_roots_mismatch");
    }
    Ok(())
}

pub(super) fn valid_root_mode(mode: u32) -> bool {
    mode <= 0o777 && mode & 0o022 == 0
}

pub(super) fn parse_root_id(value: &str) -> Option<CacheRootId> {
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

pub(super) fn root_id_text(id: CacheRootId) -> &'static str {
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

pub(super) fn root_kind(id: CacheRootId) -> &'static str {
    match id {
        CacheRootId::CargoCrates2Json | CacheRootId::CargoCratesToml => "file",
        _ => "directory",
    }
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
