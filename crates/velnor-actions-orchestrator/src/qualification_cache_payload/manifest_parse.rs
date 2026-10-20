use super::root::{parse_root, root_id_text};
use super::{
    MAX_MANIFEST_BYTES, NAMESPACE, NORMAL_NAMESPACE, PayloadManifest, PayloadScope, normal_tofu,
};
use serde_json::{Map, Value};
use velnor_actions_contract::{canonical_json_str, strict_json::parse_strict_json_with_limit};

pub(crate) fn parse_canonical(text: &str) -> Result<PayloadManifest, &'static str> {
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

pub(super) fn parse_value(value: &Value) -> Result<PayloadManifest, &'static str> {
    let object = value
        .as_object()
        .ok_or("qualification_cache_manifest_invalid")?;
    if string_field(object, "schema")? != schema() {
        return Err("qualification_cache_manifest_schema_invalid");
    }
    let namespace = string_field(object, "namespace")?;
    let scope = match namespace {
        NAMESPACE
            if has_exact_keys(
                object,
                &[
                    "cache_key",
                    "campaign",
                    "layer",
                    "owner_id",
                    "namespace",
                    "roots",
                    "runtime_identity_digest",
                    "schema",
                    "state_digest",
                ],
            ) =>
        {
            PayloadScope::Qualification {
                campaign: string_field(object, "campaign")?.to_owned(),
                owner_id: string_field(object, "owner_id")?.to_owned(),
                layer: string_field(object, "layer")?.to_owned(),
            }
        }
        NORMAL_NAMESPACE
            if has_exact_keys(
                object,
                &[
                    "cache_key",
                    "layer",
                    "namespace",
                    "roots",
                    "runtime_identity_digest",
                    "schema",
                    "state_digest",
                ],
            ) && string_field(object, "layer")? == "mise_tools" =>
        {
            PayloadScope::NormalTools
        }
        "normal_tofu_providers" => normal_tofu::parse_scope(object)?,
        _ => return Err("qualification_cache_manifest_invalid"),
    };
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
        scope,
        cache_key: string_field(object, "cache_key")?.to_owned(),
        runtime_identity_digest: string_field(object, "runtime_identity_digest")?.to_owned(),
        state_digest,
        roots,
    })
}

pub(crate) fn schema() -> String {
    format!(
        "velnor.cache-payload/v{}",
        velnor_actions_contract::workflow::QUALIFICATION_CACHE_FORMAT_VERSION
    )
}

pub(crate) fn validate_digest(value: &str) -> Result<(), &'static str> {
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
