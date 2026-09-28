//! Pinned action refs, override schema, and input validation cases.
use std::collections::BTreeMap;
use velnor_actions_actionlint::{
    ALINT_REVIEWED_TAG, ALLOWED_ACTIONS, ActionInputSchema, ActionPinOverride, ActionlintError,
    ApprovedPinCatalog, PinnedActionRef, validate_action_inputs,
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
fn allowlist_has_eight_entries() {
    assert_eq!(ALLOWED_ACTIONS.len(), 8);
    assert!(ALLOWED_ACTIONS.contains(&"actions/cache/restore"));
    assert!(ALLOWED_ACTIONS.contains(&"asamarts/alint"));
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
fn alint_tag_accepted() {
    let uses = format!("asamarts/alint@{ALINT_REVIEWED_TAG}");
    let parsed = PinnedActionRef::parse_uses(&uses, ALINT_REVIEWED_TAG);
    assert_eq!(parsed, Ok(PinnedActionRef::alint()));
}

#[test]
fn alint_wrong_tag_rejected() {
    assert!(matches!(
        PinnedActionRef::parse_uses("asamarts/alint@v0.17.0", "v0.17.0"),
        Err(ActionlintError::InvalidPin { .. })
    ));
}

#[test]
fn tag_exception_rejected_for_ordinary_action() {
    let reference = PinnedActionRef {
        repo: "actions/checkout".to_owned(),
        path: None,
        sha: None,
        version_comment: CHECKOUT_VERSION.to_owned(),
        tag_exception: true,
    };
    assert!(matches!(
        reference.validate(),
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
        assert_eq!(reference.sha, Some(CHECKOUT_SHA.to_owned()));
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
fn override_alint_rejected() {
    let catalog = ApprovedPinCatalog::new();
    let request = ActionPinOverride {
        action: "asamarts/alint".to_owned(),
        sha: CHECKOUT_SHA.to_owned(),
        version: ALINT_REVIEWED_TAG.to_owned(),
    };
    assert!(matches!(
        catalog.validate_override(&request),
        Err(ActionlintError::OverrideRejected { .. })
    ));
    let mut catalog = ApprovedPinCatalog::new();
    assert!(
        catalog
            .insert("asamarts/alint", CHECKOUT_SHA, ALINT_REVIEWED_TAG)
            .is_err()
    );
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
