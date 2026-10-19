//! Serde defaults and range checks for `[workflow]` scalar settings.
//!
//! Covers the crate-job timeout, Mise silencing, and the cache settings.
//! The Cache seed workflow is the only writer of the dependency seed
//! (`rsdeps`) family. Its jobs get their own literal timeout, every saved
//! generation stays at or below a raw-byte ceiling, and the generation-time
//! budget check proves the projected hot set against the cache quota.

use super::WorkflowConfig;
use crate::errors::ContractError;

/// Default crate-job timeout: the shared five-minute job default.
#[must_use]
pub const fn default_crate_timeout_minutes() -> u16 {
    crate::workflow::timeout::JobTimeout::DEFAULT_MINUTES
}

/// Serialization predicate keeping the default crate timeout out of emitted config.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
pub(super) fn is_default_crate_timeout(value: &u16) -> bool {
    *value == default_crate_timeout_minutes()
}

/// Default `mise_quiet` value: silence stays on.
pub(super) fn default_true() -> bool {
    true
}

/// Serialization predicate keeping the `mise_quiet` default out of emitted config.
///
/// Serde's `skip_serializing_if` requires `fn(&bool) -> bool`, so the
/// small-`Copy` by-value clippy lint does not apply here.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&bool) -> bool"
)]
pub(super) fn is_true(value: &bool) -> bool {
    *value
}

/// Default literal timeout of one Cache seed job.
pub const DEFAULT_SEED_TIMEOUT_MINUTES: u16 = 45;
/// Default raw-byte ceiling of one dependency seed generation (8 GiB).
pub const DEFAULT_SEED_MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Default repository Actions cache quota (GitHub's 10 GB).
pub const DEFAULT_CACHE_QUOTA_BYTES: u64 = 10_000_000_000;

/// Serde default for `workflow.seed_timeout_minutes`.
#[must_use]
pub(super) const fn default_seed_timeout_minutes() -> u16 {
    DEFAULT_SEED_TIMEOUT_MINUTES
}

/// Serde default for `workflow.seed_max_bytes`.
#[must_use]
pub(super) const fn default_seed_max_bytes() -> u64 {
    DEFAULT_SEED_MAX_BYTES
}

/// Serde default for `workflow.cache_quota_bytes`.
#[must_use]
pub(super) const fn default_cache_quota_bytes() -> u64 {
    DEFAULT_CACHE_QUOTA_BYTES
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
pub(super) fn is_default_seed_timeout(value: &u16) -> bool {
    *value == DEFAULT_SEED_TIMEOUT_MINUTES
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u64) -> bool"
)]
pub(super) fn is_default_seed_max_bytes(value: &u64) -> bool {
    *value == DEFAULT_SEED_MAX_BYTES
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u64) -> bool"
)]
pub(super) fn is_default_cache_quota_bytes(value: &u64) -> bool {
    *value == DEFAULT_CACHE_QUOTA_BYTES
}

/// Validate the cache settings; out-of-range values are rejected, never clamped.
/// # Errors
///
/// Returns a config error naming the offending key.
pub(super) fn validate(config: &WorkflowConfig, file: &str) -> Result<(), ContractError> {
    crate::workflow::timeout::JobTimeout::check_config(
        u32::from(config.seed_timeout_minutes),
        file,
        "workflow.seed_timeout_minutes",
    )?;
    if config.seed_max_bytes == 0 {
        return Err(ContractError::config(
            file,
            "workflow.seed_max_bytes",
            "must_be_positive",
        ));
    }
    if config.cache_quota_bytes == 0 {
        return Err(ContractError::config(
            file,
            "workflow.cache_quota_bytes",
            "must_be_positive",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::WorkflowConfig;

    fn config(extra: &[(&str, serde_json::Value)]) -> Result<WorkflowConfig, String> {
        let mut value = serde_json::json!({
            "name": "CI",
            "policy": "consumer-v1",
            "generator_validation": "bootstrap",
            "max_parallel_jobs": 2,
        });
        for (key, field) in extra {
            value[*key] = field.clone();
        }
        let config: WorkflowConfig =
            serde_json::from_value(value).map_err(|err| err.to_string())?;
        config
            .validate("config.toml")
            .map_err(|err| err.to_string())?;
        Ok(config)
    }

    #[test]
    fn cache_settings_default_and_reject_out_of_range_values() {
        let defaults = config(&[]).expect("defaults");
        assert_eq!(defaults.seed_timeout_minutes, 45);
        assert_eq!(defaults.seed_max_bytes, 8 * 1024 * 1024 * 1024);
        assert_eq!(defaults.cache_quota_bytes, 10_000_000_000);
        let set = config(&[
            ("seed_timeout_minutes", 360.into()),
            ("seed_max_bytes", 1.into()),
            ("cache_quota_bytes", 5.into()),
        ])
        .expect("explicit");
        assert_eq!(
            (
                set.seed_timeout_minutes,
                set.seed_max_bytes,
                set.cache_quota_bytes
            ),
            (360, 1, 5)
        );
        for (key, bad) in [
            ("seed_timeout_minutes", 0),
            ("seed_timeout_minutes", 361),
            ("seed_max_bytes", 0),
            ("cache_quota_bytes", 0),
        ] {
            let err = config(&[(key, bad.into())]).expect_err(key);
            assert!(err.contains(&format!("workflow.{key}")), "{key}: {err}");
        }
        assert!(config(&[("pull_request_cache_policy", "read-only".into())]).is_err());
    }
}
