//! Pinned action refs, override schema, and input validation cases.
use std::collections::BTreeMap;
use velnor_actions_actionlint::{
    ALINT_ACTION_SHA, ALINT_ACTION_VERSION, ALLOWED_ACTIONS, ActionInputSchema, ActionPinOverride,
    ActionlintError, ApprovedPinCatalog, PinnedActionRef, validate_action_inputs,
};

/// Verified checkout pin from the version policy (test fixture).
const CHECKOUT_SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";
/// Verified checkout release (test fixture).
const CHECKOUT_VERSION: &str = "v7.0.1";

fn checkout_schema() -> ActionInputSchema {
    ActionInputSchema {
        action: "actions/checkout".to_owned(),
        required: vec!["persist-credentials".to_owned()],
        optional: vec!["ref".to_owned(), "fetch-depth".to_owned()],
    }
}

#[test]
fn allowlist_has_nine_entries() {
    assert_eq!(ALLOWED_ACTIONS.len(), 9);
    assert!(ALLOWED_ACTIONS.contains(&"actions/cache/restore"));
    assert!(ALLOWED_ACTIONS.contains(&"asamarts/alint"));
    assert!(ALLOWED_ACTIONS.contains(&"Swatinem/rust-cache"));
}

#[test]
fn parse_valid_checkout_uses() {
    let uses = format!("actions/checkout@{CHECKOUT_SHA}");
    let parsed = PinnedActionRef::parse_uses(&uses, CHECKOUT_VERSION);
    assert!(parsed.is_ok());
    if let Ok(reference) = parsed {
        assert_eq!(reference.uses_key(), "actions/checkout");
        assert_eq!(
            reference.render_uses(),
            format!("uses: actions/checkout@{CHECKOUT_SHA} # {CHECKOUT_VERSION}")
        );
    }
}

#[test]
fn parse_subpath_uses() {
    let sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
    let uses = format!("actions/cache/restore@{sha}");
    let parsed = PinnedActionRef::parse_uses(&uses, "v6.1.0");
    assert!(parsed.is_ok());
    if let Ok(reference) = parsed {
        assert_eq!(reference.uses_key(), "actions/cache/restore");
        assert_eq!(reference.path, Some("restore".to_owned()));
    }
}

#[test]
fn branch_ref_rejected() {
    assert!(matches!(
        PinnedActionRef::parse_uses("actions/checkout@v7.0.1", "v7.0.1"),
        Err(ActionlintError::InvalidPin { .. })
    ));
}

#[test]
fn forbidden_installer_actions_rejected() {
    let sha = "c2a87611a18de5b3828c5652fe268e992400cb5c";
    assert!(matches!(
        PinnedActionRef::parse_uses(&format!("actions/setup-go@{sha}"), "v5.0.0"),
        Err(ActionlintError::UnknownAction { .. })
    ));
    assert!(matches!(
        PinnedActionRef::parse_uses(&format!("taiki-e/install-action@{sha}"), "v2.0.0"),
        Err(ActionlintError::UnknownAction { .. })
    ));
}

#[test]
fn alint_sha_accepted() {
    let uses = format!("asamarts/alint@{ALINT_ACTION_SHA}");
    let parsed = PinnedActionRef::parse_uses(&uses, ALINT_ACTION_VERSION);
    assert_eq!(parsed, Ok(PinnedActionRef::alint()));
}

#[test]
fn alint_tag_refs_rejected() {
    for (uses, comment) in [
        (
            format!("asamarts/alint@{ALINT_ACTION_VERSION}"),
            ALINT_ACTION_VERSION,
        ),
        ("asamarts/alint@v0.17.0".to_owned(), "v0.17.0"),
    ] {
        assert!(
            matches!(
                PinnedActionRef::parse_uses(&uses, comment),
                Err(ActionlintError::InvalidPin { .. })
            ),
            "accepted {uses} # {comment}"
        );
    }
}

#[test]
fn malformed_sha_rejected_for_ordinary_action() {
    assert!(matches!(
        PinnedActionRef::new("actions/checkout", None, "abc123", CHECKOUT_VERSION),
        Err(ActionlintError::InvalidPin { .. })
    ));
    assert!(matches!(
        PinnedActionRef::new("actions/checkout", None, CHECKOUT_SHA, "v7",),
        Err(ActionlintError::InvalidPin { .. })
    ));
}

#[test]
fn override_approved_pair_accepted() {
    let mut catalog = ApprovedPinCatalog::new();
    assert_eq!(
        catalog.insert("actions/checkout", CHECKOUT_SHA, CHECKOUT_VERSION),
        Ok(())
    );
    let request = ActionPinOverride {
        action: "actions/checkout".to_owned(),
        sha: CHECKOUT_SHA.to_owned(),
        version: CHECKOUT_VERSION.to_owned(),
    };
    let validated = catalog.validate_override(&request);
    assert!(validated.is_ok());
    if let Ok(reference) = validated {
        assert_eq!(reference.sha, CHECKOUT_SHA);
        assert_eq!(reference.version_comment, CHECKOUT_VERSION);
    }
}

#[test]
fn override_unapproved_pair_rejected() {
    let catalog = ApprovedPinCatalog::new();
    let request = ActionPinOverride {
        action: "actions/checkout".to_owned(),
        sha: CHECKOUT_SHA.to_owned(),
        version: CHECKOUT_VERSION.to_owned(),
    };
    assert!(matches!(
        catalog.validate_override(&request),
        Err(ActionlintError::OverrideRejected { .. })
    ));
}

#[test]
fn override_unknown_action_rejected() {
    let catalog = ApprovedPinCatalog::new();
    let request = ActionPinOverride {
        action: "octo/unknown".to_owned(),
        sha: CHECKOUT_SHA.to_owned(),
        version: CHECKOUT_VERSION.to_owned(),
    };
    assert!(matches!(
        catalog.validate_override(&request),
        Err(ActionlintError::UnknownAction { .. })
    ));
}

#[test]
fn override_alint_approved_pair_rejected() {
    // The Alint pin is policy-owned, not consumer-overridable
    // (docs/content/docs/proposed/version-policy.mdx §2 (GitHub Action defaults)).
    let mut catalog = ApprovedPinCatalog::new();
    assert!(matches!(
        catalog.insert("asamarts/alint", ALINT_ACTION_SHA, ALINT_ACTION_VERSION),
        Err(ActionlintError::OverrideRejected { .. })
    ));
    for (sha, version) in [
        (ALINT_ACTION_SHA, ALINT_ACTION_VERSION),
        (CHECKOUT_SHA, ALINT_ACTION_VERSION),
    ] {
        let request = ActionPinOverride {
            action: "asamarts/alint".to_owned(),
            sha: sha.to_owned(),
            version: version.to_owned(),
        };
        assert!(matches!(
            catalog.validate_override(&request),
            Err(ActionlintError::OverrideRejected { .. })
        ));
    }
}

#[test]
fn inputs_valid_accepted() {
    let inputs = BTreeMap::from([
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("fetch-depth".to_owned(), "1".to_owned()),
    ]);
    assert_eq!(validate_action_inputs(&checkout_schema(), &inputs), Ok(()));
}

#[test]
fn inputs_unknown_rejected() {
    let inputs = BTreeMap::from([
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("injected".to_owned(), "true".to_owned()),
    ]);
    assert!(matches!(
        validate_action_inputs(&checkout_schema(), &inputs),
        Err(ActionlintError::UnknownActionInput { .. })
    ));
}

#[test]
fn inputs_missing_required_rejected() {
    let inputs = BTreeMap::from([("ref".to_owned(), "abc".to_owned())]);
    assert!(matches!(
        validate_action_inputs(&checkout_schema(), &inputs),
        Err(ActionlintError::MissingActionInput { .. })
    ));
}

#[test]
fn inputs_multiline_value_rejected() {
    let inputs = BTreeMap::from([(
        "persist-credentials".to_owned(),
        "false\nrun: evil".to_owned(),
    )]);
    assert!(matches!(
        validate_action_inputs(&checkout_schema(), &inputs),
        Err(ActionlintError::InvalidActionInput { .. })
    ));
}

#[test]
fn rust_cache_full_sha_pin_accepted() {
    use velnor_actions_actionlint::{RUST_CACHE_ACTION_SHA, RUST_CACHE_ACTION_VERSION};
    let uses = format!("Swatinem/rust-cache@{RUST_CACHE_ACTION_SHA}");
    let parsed = PinnedActionRef::parse_uses(&uses, RUST_CACHE_ACTION_VERSION);
    assert!(parsed.is_ok(), "qualified pin must parse: {uses}");
    assert_eq!(RUST_CACHE_ACTION_SHA.len(), 40);
}

#[test]
fn rust_cache_moving_refs_rejected() {
    for (uses, comment) in [
        ("Swatinem/rust-cache@v2.9.2", "v2.9.2"),
        ("Swatinem/rust-cache@main", "v2.9.2"),
        ("Swatinem/rust-cache@6323deb", "v2.9.2"),
    ] {
        assert!(
            PinnedActionRef::parse_uses(uses, comment).is_err(),
            "must reject {uses}"
        );
    }
}

#[test]
fn rust_cache_inputs_schema_enforced() {
    use velnor_actions_actionlint::rust_cache_inputs_schema;
    let schema = rust_cache_inputs_schema();
    let good = BTreeMap::from([
        ("shared-key".to_owned(), "velnor-cargo-x".to_owned()),
        ("save-if".to_owned(), "false".to_owned()),
        ("cache-targets".to_owned(), "false".to_owned()),
        ("cache-on-failure".to_owned(), "false".to_owned()),
    ]);
    assert_eq!(validate_action_inputs(&schema, &good), Ok(()));
    let missing = BTreeMap::from([("shared-key".to_owned(), "x".to_owned())]);
    assert!(validate_action_inputs(&schema, &missing).is_err());
    let injected = BTreeMap::from([
        ("shared-key".to_owned(), "x".to_owned()),
        ("save-if".to_owned(), "false".to_owned()),
        ("cache-targets".to_owned(), "false".to_owned()),
        ("cache-on-failure".to_owned(), "false".to_owned()),
        ("workspaces".to_owned(), ".".to_owned()),
    ]);
    assert!(validate_action_inputs(&schema, &injected).is_err());
    let empty = BTreeMap::from([
        ("shared-key".to_owned(), String::new()),
        ("save-if".to_owned(), "false".to_owned()),
        ("cache-targets".to_owned(), "false".to_owned()),
        ("cache-on-failure".to_owned(), "false".to_owned()),
    ]);
    assert!(validate_action_inputs(&schema, &empty).is_err());
}

#[test]
fn mbx_experiment_target_is_current_unqualified_release() {
    use velnor_actions_actionlint::actions::{
        MR_BOXINGTON_ACTION_CANDIDATE_SHA, MR_BOXINGTON_ACTION_CANDIDATE_VERSION,
        MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION,
    };

    assert_ne!(
        (
            MR_BOXINGTON_ACTION_CANDIDATE_VERSION,
            MR_BOXINGTON_ACTION_CANDIDATE_SHA
        ),
        (MR_BOXINGTON_ACTION_VERSION, MR_BOXINGTON_ACTION_SHA),
        "the experiment target must stay separate from the production pin"
    );
    let inventory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.velnor/freshness-inventory.json");
    let inventory = std::fs::read_to_string(inventory).expect("freshness inventory readable");
    let action = inventory
        .split("\"key\": \"jdx/mr-boxington-action\"")
        .nth(1)
        .expect("MBX action inventory row present");
    let action = action
        .split_once("\n    }")
        .map(|(row, _)| row)
        .expect("MBX action inventory row terminates");
    assert!(
        action.contains(&format!(
            "\"latest\": \"{MR_BOXINGTON_ACTION_CANDIDATE_VERSION}\""
        )),
        "candidate version must match the current inventory row: {action}"
    );
    assert!(
        action.contains(&format!(
            "\"latest_sha\": \"{MR_BOXINGTON_ACTION_CANDIDATE_SHA}\""
        )),
        "candidate SHA must match the current inventory row: {action}"
    );
}
