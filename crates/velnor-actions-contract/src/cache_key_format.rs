//! One authoritative namespace per cache transport policy.

use crate::{ContractError, validate_digest};

/// Isolated, direct-only public-provider payloads never read the older namespace.
pub const TOFU_PROVIDERS_KEY_PREFIX: &str = "velnor-v2-tofu-providers";

fn layer_prefix(layer: &str) -> Result<&'static str, ContractError> {
    match layer {
        "sources" => Ok("velnor-v1-sources"),
        "mbx" => Ok("velnor-v1-mbx"),
        "task" => Ok("velnor-v1-task"),
        "tofu-providers" => Ok(TOFU_PROVIDERS_KEY_PREFIX),
        _ => Err(ContractError::identity("cache.layer", "unknown_layer")),
    }
}

fn validate_trust(trust: &str) -> Result<(), ContractError> {
    if matches!(trust, "trusted" | "pr") {
        Ok(())
    } else {
        Err(ContractError::identity("cache.trust", "unknown_trust"))
    }
}

/// Build the layer's current `<prefix>-<trust>-<compat>-<snapshot>` key.
/// # Errors
pub fn cache_key(
    layer: &str,
    trust: &str,
    compatibility: &str,
    snapshot: &str,
) -> Result<String, ContractError> {
    let prefix = layer_prefix(layer)?;
    validate_trust(trust)?;
    validate_digest(compatibility)?;
    validate_digest(snapshot)?;
    let key = format!("{prefix}-{trust}-{compatibility}-{snapshot}");
    if key.len() > super::MAX_CACHE_KEY_BYTES {
        return Err(ContractError::identity("cache.key", "key_too_long"));
    }
    Ok(key)
}

/// Build a same-compatibility prefix within the layer's current namespace.
/// # Errors
pub fn restore_prefix(
    layer: &str,
    trust: &str,
    compatibility: &str,
) -> Result<String, ContractError> {
    let prefix = layer_prefix(layer)?;
    validate_trust(trust)?;
    validate_digest(compatibility)?;
    Ok(format!("{prefix}-{trust}-{compatibility}-"))
}
