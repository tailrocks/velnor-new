//! Contract action-override allowlist cases (split from `impl_contract.rs`
//! to satisfy the Alint 400-line gate, rust-quality-contract §5).
use std::collections::BTreeMap;
use velnor_actions_contract::ContractError;

#[test]
fn actions_overrides_validate_allowlist_and_pin_shape() {
    use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
    let sha = "3d3c42e5aac5ba805825da76410c181273ba90b1";
    let good = ActionsConfig {
        overrides: BTreeMap::from([(
            "actions/checkout".to_owned(),
            ActionPinOverride {
                sha: sha.to_owned(),
                version: "v7.0.1".to_owned(),
            },
        )]),
    };
    assert_eq!(good.validate("cfg"), Ok(()));
    assert_eq!(ActionsConfig::default().validate("cfg"), Ok(()));
    let alint = ActionsConfig {
        overrides: BTreeMap::from([(
            "asamarts/alint".to_owned(),
            ActionPinOverride {
                sha: "9f9d34ba0eae3888299b9e570f43338b0e7f2cdb".to_owned(),
                version: "v0.16.1".to_owned(),
            },
        )]),
    };
    // The Alint pin is policy-owned, not consumer-overridable
    // (docs/proposed/version-policy.md §2.3).
    assert!(matches!(
        alint.validate("cfg"),
        Err(ContractError::Config { problem, .. }) if problem == "unknown_action"
    ));
    for (action, pin, problem) in [
        (
            "bogus/action",
            ActionPinOverride {
                sha: sha.to_owned(),
                version: "v1.2.3".to_owned(),
            },
            "unknown_action",
        ),
        (
            "actions/checkout",
            ActionPinOverride {
                sha: "abc123".to_owned(),
                version: "v7.0.1".to_owned(),
            },
            "ref_must_be_full_sha",
        ),
        (
            "actions/checkout",
            ActionPinOverride {
                sha: sha.to_owned(),
                version: "v7".to_owned(),
            },
            "invalid_version:v7",
        ),
    ] {
        let config = ActionsConfig {
            overrides: BTreeMap::from([(action.to_owned(), pin)]),
        };
        let Err(velnor_actions_contract::ContractError::Config {
            key_path,
            problem: got,
            ..
        }) = config.validate("cfg")
        else {
            panic!("override {action} must be rejected");
        };
        assert_eq!(key_path, format!("actions.overrides.{action}"));
        assert_eq!(got, problem);
    }
}
