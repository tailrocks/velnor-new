//! Typed step authority stays independent of presentation names.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::validate_step_sequence;
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

const TOFU_KEY: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-0123456789ab-${{hashFiles('.terraform.lock.hcl')}}";
const TOFU_PATH: &str = "${{ runner.temp }}/velnor/tofu-cache/root-0123456789ab";
const TOFU_DATA_PATH: &str = "${{ runner.temp }}/velnor/tofu-data/root-0123456789ab";
const OTHER_TOFU_KEY: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.2-stacks-vpc-abcdef012345-${{hashFiles('stacks/vpc/.terraform.lock.hcl')}}";
const OTHER_TOFU_PATH: &str = "${{ runner.temp }}/velnor/tofu-cache/stacks-vpc-abcdef012345";

fn plan_step(name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: Some(StepId::Plan),
        role: Some(StepRole::PlanProducer),
        condition: None,
        kind: StepKind::Internal {
            operation: "plan-v1".to_owned(),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn display_name_does_not_change_typed_output_identity() {
    let mut step = plan_step("Plan");
    validate_step_sequence(std::slice::from_ref(&step), "plan").expect("valid plan owner");

    step.name = "A presentation-only label".to_owned();
    validate_step_sequence(std::slice::from_ref(&step), "plan").expect("rename preserves role");
    assert_eq!(step.id, Some(StepId::Plan));
    assert_eq!(step.role, Some(StepRole::PlanProducer));
}

#[test]
fn final_serialized_scope_rejects_duplicate_typed_ids() {
    let steps = [plan_step("Plan"), plan_step("Renamed Plan")];
    let error = validate_step_sequence(&steps, "plan").expect_err("duplicate output id");
    assert!(error.to_string().contains("duplicate_step_id:plan:plan"));
}

#[test]
fn role_kind_mismatch_fails_even_when_display_name_matches() {
    let step = Step {
        name: "Plan".to_owned(),
        id: None,
        role: Some(StepRole::PlanProducer),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let error = validate_step_sequence(&[step], "plan").expect_err("wrong semantic payload");
    assert!(error.to_string().contains("role_kind_mismatch"));
}

fn tofu_restore() -> Step {
    Step {
        name: "Restore Tofu providers".to_owned(),
        id: Some(StepId::TofuProviders),
        role: Some(StepRole::TofuProvidersRestore),
        condition: None,
        kind: StepKind::Action {
            uses: velnor_actions_contract::workflow::step_identity::TOFU_PROVIDER_ADMISSION_USES
                .to_owned(),
            with: BTreeMap::from([
                ("cache-key".to_owned(), TOFU_KEY.to_owned()),
                ("cache-path".to_owned(), TOFU_PATH.to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    }
}

fn tofu_use() -> Step {
    Step {
        name: "OpenTofu init and validate".to_owned(),
        id: None,
        role: Some(StepRole::TofuProviderUse),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["tofu".to_owned(), "init".to_owned()],
            env: BTreeMap::from([
                ("TF_PLUGIN_CACHE_DIR".to_owned(), TOFU_PATH.to_owned()),
                ("TF_DATA_DIR".to_owned(), TOFU_DATA_PATH.to_owned()),
            ]),
        },
    }
}

fn tofu_save() -> Step {
    Step {
        name: "Save Tofu providers".to_owned(),
        id: None,
        role: Some(StepRole::TofuProvidersSave),
        condition: Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned()),
        kind: StepKind::Action {
            uses: "actions/cache/save@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
            with: BTreeMap::from([
                (
                    "key".to_owned(),
                    velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR
                        .to_owned(),
                ),
                (
                    "path".to_owned(),
                    velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_PATH_OUTPUT_EXPR
                        .to_owned(),
                ),
            ]),
            env: BTreeMap::new(),
        },
    }
}

fn tofu_sequence() -> Vec<Step> {
    vec![tofu_restore(), tofu_use(), tofu_save()]
}

#[test]
fn tofu_provider_pair_and_consumer_sequence_is_valid() {
    validate_step_sequence(&tofu_sequence(), "tofu-job")
        .expect("composite restore/admission, use, and save are bound");
}

#[test]
fn tofu_provider_save_must_follow_restore_without_consumers() {
    let steps = [tofu_save(), tofu_restore()];
    assert!(
        validate_step_sequence(&steps, "tofu-job")
            .expect_err("provider save before restore is rejected without consumers")
            .to_string()
            .contains("tofu_provider_save_before_restore")
    );
}

#[test]
fn tofu_provider_sequence_rejects_missing_conditional_or_malformed_composite() {
    let mut missing = tofu_sequence();
    missing.remove(0);
    assert!(
        validate_step_sequence(&missing, "tofu-job")
            .expect_err("provider use without restore/admission composite")
            .to_string()
            .contains("tofu_provider_restore_count")
    );

    let mut conditional = tofu_sequence();
    conditional[0].condition = Some("success()".to_owned());
    assert!(
        validate_step_sequence(&conditional, "tofu-job")
            .expect_err("conditional restore/admission composite")
            .to_string()
            .contains("tofu_provider_restore_conditional")
    );

    let mut wrong_key_slug = tofu_sequence();
    if let StepKind::Action { with, .. } = &mut wrong_key_slug[0].kind {
        with.insert("cache-key".to_owned(), OTHER_TOFU_KEY.to_owned());
    }
    assert!(
        validate_step_sequence(&wrong_key_slug, "tofu-job")
            .expect_err("composite key must bind the path leaf")
            .to_string()
            .contains("role_kind_mismatch")
    );

    let mut wrong_path_slug = tofu_sequence();
    if let StepKind::Action { with, .. } = &mut wrong_path_slug[0].kind {
        with.insert("cache-path".to_owned(), OTHER_TOFU_PATH.to_owned());
    }
    assert!(
        validate_step_sequence(&wrong_path_slug, "tofu-job")
            .expect_err("composite path must bind the key leaf")
            .to_string()
            .contains("role_kind_mismatch")
    );
}

#[test]
fn tofu_provider_sequence_rejects_unadmitted_or_misdirected_use() {
    let mut before_restore = tofu_sequence();
    let consumer = before_restore.remove(1);
    before_restore.insert(0, consumer);
    assert!(
        validate_step_sequence(&before_restore, "tofu-job")
            .expect_err("consumer before admission")
            .to_string()
            .contains("tofu_provider_use_before_admission")
    );

    let mut wrong_path = tofu_sequence();
    if let StepKind::Shell { env, .. } = &mut wrong_path[1].kind {
        env.insert("TF_PLUGIN_CACHE_DIR".to_owned(), OTHER_TOFU_PATH.to_owned());
    }
    assert!(
        validate_step_sequence(&wrong_path, "tofu-job")
            .expect_err("consumer must use the admitted path")
            .to_string()
            .contains("tofu_provider_use_path_mismatch")
    );

    let mut nested_data_dir = tofu_sequence();
    if let StepKind::Shell { env, .. } = &mut nested_data_dir[1].kind {
        env.insert(
            "TF_DATA_DIR".to_owned(),
            format!("{TOFU_PATH}/terraform-data"),
        );
    }
    assert!(
        validate_step_sequence(&nested_data_dir, "tofu-job")
            .expect_err("plugin cache may not contain the data directory")
            .to_string()
            .contains("role_kind_mismatch")
    );

    let mut untyped_use = tofu_sequence();
    untyped_use[1].role = None;
    assert!(
        validate_step_sequence(&untyped_use, "tofu-job")
            .expect_err("provider environment requires a typed consumer")
            .to_string()
            .contains("tofu_provider_use_role_missing")
    );
}

#[test]
fn tofu_provider_save_must_match_restore_gate_and_order() {
    let mut wrong_key = tofu_sequence();
    if let StepKind::Action { with, .. } = &mut wrong_key[2].kind {
        with.insert("key".to_owned(), OTHER_TOFU_KEY.to_owned());
        with.insert("path".to_owned(), OTHER_TOFU_PATH.to_owned());
    }
    assert!(validate_step_sequence(&wrong_key, "tofu-job").is_err());

    let mut wrong_output_owner = tofu_sequence();
    if let StepKind::Action { with, .. } = &mut wrong_output_owner[2].kind {
        with.insert(
            "key".to_owned(),
            "${{ steps.another-step.outputs.cache-key }}".to_owned(),
        );
    }
    assert!(validate_step_sequence(&wrong_output_owner, "tofu-job").is_err());

    let mut weak_gate = tofu_sequence();
    weak_gate[2].condition = Some("success()".to_owned());
    assert!(
        validate_step_sequence(&weak_gate, "tofu-job")
            .expect_err("save must keep the elected push gate")
            .to_string()
            .contains("tofu_provider_save_gate_mismatch")
    );

    let mut duplicate = tofu_sequence();
    duplicate.push(tofu_save());
    assert!(
        validate_step_sequence(&duplicate, "tofu-job")
            .expect_err("provider save must be unique")
            .to_string()
            .contains("tofu_provider_save_count")
    );

    let mut before_use = tofu_sequence();
    let save = before_use.pop().expect("save exists");
    before_use.insert(1, save);
    assert!(
        validate_step_sequence(&before_use, "tofu-job")
            .expect_err("save must follow every provider consumer")
            .to_string()
            .contains("tofu_provider_save_before_use")
    );
}

#[test]
fn configured_checkout_authority_is_typed_exact_and_name_independent() {
    use velnor_actions_contract::workflow::step_identity::is_configured_checkout;

    const CHECKOUT_USES: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";
    let mut checkout = Step {
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
