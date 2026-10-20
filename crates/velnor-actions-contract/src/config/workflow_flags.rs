
//! Serde helpers for boolean `[workflow]` flags.

/// Default `mise_quiet` value: silence stays on.
pub(super) fn default_true() -> bool {
    true
}

/// Serialization predicate keeping `false` defaults out of emitted config.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&bool) -> bool"
)]
pub(super) fn is_false(value: &bool) -> bool {
    !*value
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

#[cfg(test)]
mod tests {
    use super::super::WorkflowConfig;

    #[test]
    fn checkout_lfs_defaults_off_and_serializes_only_when_enabled() {
        let value = serde_json::json!({
            "name": "CI",
            "policy": "consumer-v1",
            "generator_validation": "bootstrap",
            "max_parallel_jobs": 2
        });
        let config: WorkflowConfig =
            serde_json::from_value(value.clone()).expect("default workflow");
        assert!(!config.checkout_lfs, "absent checkout_lfs must default off");
        assert_eq!(
            serde_json::to_value(&config)
                .expect("serialize workflow")
                .get("checkout_lfs"),
            None,
            "the checkout_lfs default must stay out of emitted config"
        );
        let mut enabled = value.clone();
        enabled["checkout_lfs"] = serde_json::Value::Bool(true);
        let config: WorkflowConfig = serde_json::from_value(enabled).expect("enabled workflow");
        assert!(config.checkout_lfs);
        assert_eq!(
            serde_json::to_value(&config).expect("serialize workflow")["checkout_lfs"],
            serde_json::Value::Bool(true)
        );
        for wire in ["\"yes\"", "1", "[]"] {
            let mut bad = value.clone();
            bad["checkout_lfs"] = serde_json::from_str(wire).expect("json wire");
            assert!(
                serde_json::from_value::<WorkflowConfig>(bad).is_err(),
                "checkout_lfs = {wire} must fail as a non-boolean"
            );
        }
    }
}
