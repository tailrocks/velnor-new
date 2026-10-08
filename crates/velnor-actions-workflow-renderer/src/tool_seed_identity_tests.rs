use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use super::*;

#[test]
fn step_key_is_derived_from_pinned_job_tools_and_configured_checkout_payload() {
    let setup = setup_config();
    assert_configured_checkout_prelude(&setup);
    assert_full_history_checkout_is_seedable(&setup);
    assert_display_name_does_not_authorize_seed(&setup);
    assert_wrong_pinned_checkout_stays_cold(&setup);
}

fn assert_configured_checkout_prelude(setup: &crate::MiseSetup) {
    let mut checkout = crate::steps::checkout_step(CHECKOUT).expect("checkout");
    checkout.name = "Fetch source".to_owned();
    let mut rendered_job = job(vec![checkout, mise_shell()]);
    crate::cache_p08::ensure_tools_cache_v2(
        "fixture",
        &mut rendered_job,
        setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("setup and seed");
    assert_eq!(
        rendered_job.steps[1].name,
        crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME
    );
    assert_eq!(
        rendered_job.steps[1].role,
        Some(StepRole::ToolsCacheIdentity)
    );
    assert_eq!(
        rendered_job.steps[1].id,
        Some(velnor_actions_contract::StepId::ToolsCacheIdentity)
    );
    let StepKind::Action { with, .. } = &rendered_job.steps[1].kind else {
        panic!("V2 identity and seed use the registered prelude composite")
    };
    assert_eq!(
        with.get(crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
            .map(String::as_str),
        Some(tools_payload().static_digest())
    );
    assert_eq!(
        rendered_job.steps[2].name,
        crate::cache_steps::TOOLS_RESTORE_NAME
    );
    assert_eq!(rendered_job.steps[3].name, "Setup Mise");
    assert_eq!(rendered_job.steps[3].role, Some(StepRole::MiseSetup));
    let action = crate::cache_p08::runtime_prelude_action_file(TARGET_RUNS, "0.1.0")
        .expect("prelude action file");
    assert!(action.bytes.contains(&cache_key()));
    assert!(action.bytes.contains("cache_key:"));
}

fn assert_full_history_checkout_is_seedable(setup: &crate::MiseSetup) {
    let mut full_history_checkout = crate::steps::checkout_step(CHECKOUT).expect("checkout");
    if let StepKind::Action { with, .. } = &mut full_history_checkout.kind {
        with.insert("fetch-depth".to_owned(), "0".to_owned());
    }
    let mut deep_job = job(vec![full_history_checkout, mise_shell()]);
    crate::cache_p08::ensure_tools_cache_v2(
        "full-history-checkout",
        &mut deep_job,
        setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("full-history checkout is an eligible seed owner");
    assert_eq!(deep_job.steps[1].role, Some(StepRole::ToolsCacheIdentity));
}

fn assert_display_name_does_not_authorize_seed(setup: &crate::MiseSetup) {
    let fake = Step {
        name: "Checkout".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let mut fake_job = job(vec![fake, mise_shell()]);
    crate::cache_p08::ensure_tools_cache_v2(
        "fake-checkout",
        &mut fake_job,
        setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("cold setup");
    assert!(
        fake_job
            .steps
            .iter()
            .all(|step| step.role != Some(StepRole::ToolsCacheIdentity))
    );
}

fn assert_wrong_pinned_checkout_stays_cold(setup: &crate::MiseSetup) {
    let mut wrong_pin =
        crate::steps::checkout_step("actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect("pinned checkout shape");
    wrong_pin.name = "Checkout".to_owned();
    let mut wrong_pin_job = job(vec![wrong_pin, mise_shell()]);
    crate::cache_p08::ensure_tools_cache_v2(
        "wrong-checkout-ref",
        &mut wrong_pin_job,
        setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("mismatched checkout remains cold");
    assert!(
        wrong_pin_job
            .steps
            .iter()
            .all(|step| step.role != Some(StepRole::ToolsCacheIdentity))
    );
}

fn setup_config() -> crate::MiseSetup {
    crate::MiseSetup {
        uses: SETUP_USES.to_owned(),
        version: "2026.10.5".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn mise_shell() -> Step {
    Step {
        name: "Run Cargo".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec![
                "mise".to_owned(),
                "exec".to_owned(),
                "rust@1.98.1".to_owned(),
                "--".to_owned(),
                "cargo".to_owned(),
                "check".to_owned(),
            ],
            env: BTreeMap::new(),
        },
    }
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Seed fixture".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        check_runner: None,
        steps,
    }
}

#[test]
fn malformed_seed_key_and_preexisting_v2_steps_are_rejected() {
    let setup = setup_config();
    let mut rendered_job = job(vec![
        crate::steps::checkout_step(CHECKOUT).expect("checkout"),
        mise_shell(),
    ]);
    crate::cache_p08::ensure_tools_cache_v2(
        "fixture",
        &mut rendered_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("setup and seed");
    let mut mismatched_key = job(vec![
        crate::steps::checkout_step(CHECKOUT).expect("checkout"),
        mise_shell(),
        Step {
            name: TOOL_SEED_NAME.to_owned(),
            id: None,
            role: Some(StepRole::ToolSeed),
            condition: None,
            kind: StepKind::Action {
                uses: TOOL_SEED_USES.to_owned(),
                with: BTreeMap::from([(
                    "cache_key".to_owned(),
                    "mise-v1-x86_64-unknown-linux-gnu-2026.10.5-0123456789abcdef".to_owned(),
                )]),
                env: BTreeMap::new(),
            },
        },
    ]);
    assert!(
        any_job_has_seed(&BTreeMap::from([(
            "wrong-seed-key".to_owned(),
            mismatched_key.clone()
        )]))
        .is_err()
    );
    assert!(
        crate::cache_p08::ensure_tools_cache_v2(
            "wrong-seed-key",
            &mut mismatched_key,
            &setup,
            false,
            TARGET,
            CHECKOUT,
        )
        .is_err()
    );

    let mut mismatched_setup = rendered_job;
    if let StepKind::Action { with, .. } = &mut mismatched_setup.steps[1].kind {
        with.insert("d".to_owned(), "not-a-lowercase-static-digest".to_owned());
    }
    assert!(
        crate::cache_p08::ensure_tools_cache_v2(
            "wrong-setup-key",
            &mut mismatched_setup,
            &setup,
            false,
            TARGET,
            CHECKOUT,
        )
        .is_err()
    );
}

#[path = "tool_seed_action_file_tests.rs"]
mod action_file_tests;
