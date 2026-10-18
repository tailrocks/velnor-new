use crate::canonical::validate_digest;
use crate::errors::ContractError;

use super::{QualificationCacheLayer, QualificationCacheSlot};

/// Construct a stable archive key from validated identity and one typed slot.
pub(crate) fn key_for_slot(
    campaign: &str,
    layer: QualificationCacheLayer,
    identity_digest: &str,
    runtime_digest: &str,
    slot: QualificationCacheSlot,
) -> Result<String, ContractError> {
    validate_digest(identity_digest)?;
    validate_digest(runtime_digest)?;
    let key = format!(
        "velnor-qcache-v{}-{campaign}-{}-{identity_digest}-{runtime_digest}-{}",
        crate::workflow::QUALIFICATION_CACHE_FORMAT_VERSION,
        layer.as_str(),
        slot.as_str()
    );
    if campaign.is_empty()
        || campaign.len() > 64
        || !campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || key.len() > crate::cachekey::MAX_CACHE_KEY_BYTES
    {
        return Err(ContractError::identity(
            "qualification.cache_key",
            "invalid_or_too_long",
        ));
    }
    Ok(key)
}
