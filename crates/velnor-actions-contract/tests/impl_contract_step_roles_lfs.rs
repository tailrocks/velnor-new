use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::{
    is_configured_checkout, validate_step_sequence,
};
use velnor_actions_contract::{Step, StepKind, StepRole};

const CHECKOUT_USES: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

#[test]
fn configured_checkout_accepts_only_enabled_lfs_input() {
    let step = |with| Step {
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: CHECKOUT_USES.to_owned(),
            with,
            env: BTreeMap::new(),
        },
    };
    let base = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    let mut lfs = base.clone();
    lfs.insert("lfs".to_owned(), "true".to_owned());
    assert!(is_configured_checkout(&step(lfs), CHECKOUT_USES));

    let mut disabled = base.clone();
    disabled.insert("lfs".to_owned(), "false".to_owned());
    assert!(!is_configured_checkout(&step(disabled), CHECKOUT_USES));

    let mut unknown = base;
    unknown.insert("sparse-checkout".to_owned(), "src".to_owned());
    assert!(!is_configured_checkout(&step(unknown), CHECKOUT_USES));
}

#[test]
fn configured_checkout_authority_is_typed_exact_and_name_independent() {
    let mut checkout = Step {
        parallel: false,
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: CHECKOUT_USES.to_owned(),
            with: BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]),
            env: BTreeMap::new(),
        },
    };
    assert!(is_configured_checkout(&checkout, CHECKOUT_USES));

    checkout.name = "Presentation label only".to_owned();
    assert!(is_configured_checkout(&checkout, CHECKOUT_USES));

    checkout.role = None;
    assert!(!is_configured_checkout(&checkout, CHECKOUT_USES));
    checkout.role = Some(StepRole::Checkout);

    assert!(!is_configured_checkout(
        &checkout,
        "actions/checkout@fedcba9876543210fedcba9876543210fedcba98"
    ));

    checkout.condition = Some("success()".to_owned());
    assert!(!is_configured_checkout(&checkout, CHECKOUT_USES));
    checkout.condition = None;

    if let StepKind::Action { with, .. } = &mut checkout.kind {
        with.insert("fetch-depth".to_owned(), "0".to_owned());
    }
    assert!(is_configured_checkout(&checkout, CHECKOUT_USES));
    validate_step_sequence(std::slice::from_ref(&checkout), "checkout")
        .expect("general checkout role supports workflow-specific inputs");

    if let StepKind::Action { with, .. } = &mut checkout.kind {
        with.insert("lfs".to_owned(), "true".to_owned());
    }
    assert!(is_configured_checkout(&checkout, CHECKOUT_USES));
    if let StepKind::Action { with, .. } = &mut checkout.kind {
        with.insert("lfs".to_owned(), "false".to_owned());
    }
    assert!(!is_configured_checkout(&checkout, CHECKOUT_USES));
    if let StepKind::Action { with, .. } = &mut checkout.kind {
        with.remove("lfs");
        with.insert("sparse-checkout".to_owned(), "src".to_owned());
    }
    assert!(!is_configured_checkout(&checkout, CHECKOUT_USES));

    if let StepKind::Action { with, .. } = &mut checkout.kind {
        with.clear();
        with.insert("persist-credentials".to_owned(), "false".to_owned());
        with.insert("fetch-depth".to_owned(), "1".to_owned());
    }
    assert!(!is_configured_checkout(&checkout, CHECKOUT_USES));
    validate_step_sequence(std::slice::from_ref(&checkout), "checkout")
        .expect("general checkout role supports workflow-specific inputs");

    if let StepKind::Action { with, env, .. } = &mut checkout.kind {
        with.remove("fetch-depth");
        env.insert("GIT_CONFIG_COUNT".to_owned(), "1".to_owned());
    }
    assert!(!is_configured_checkout(&checkout, CHECKOUT_USES));
    validate_step_sequence(std::slice::from_ref(&checkout), "checkout")
        .expect("general checkout role supports workflow-specific environment");
}
