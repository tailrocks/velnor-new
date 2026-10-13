use super::super::*;
use super::{CHECKOUT, SETUP_USES, TARGET, cache_key};
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::{Job, Step, StepKind, StepRole};

#[test]
fn step_key_is_derived_from_pinned_job_tools_and_configured_checkout_payload() {
    let setup = setup_config();
    let expected = cache_key();
    let mut checkout = crate::steps::checkout_step(CHECKOUT).expect("checkout");
    checkout.name = "Fetch source".to_owned();
    let mut rendered_job = job(vec![checkout, mise_shell()]);
    crate::cache_p08::ensure_setup_p08(
        "fixture",
        &mut rendered_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("setup and seed");
    assert_eq!(rendered_job.steps[1].name, TOOL_SEED_NAME);
    assert_eq!(rendered_job.steps[1].role, Some(StepRole::ToolSeed));
    let StepKind::Action { with, .. } = &rendered_job.steps[1].kind else {
        panic!("seed must be an action")
    };
    assert_eq!(
        with.get("cache_key").map(String::as_str),
        Some(expected.as_str())
    );
    assert_eq!(rendered_job.steps[2].name, "Setup Mise");
    assert_eq!(rendered_job.steps[2].role, Some(StepRole::MiseSetup));

    let mut full_history_checkout = crate::steps::checkout_step(CHECKOUT).expect("checkout");
    if let StepKind::Action { with, .. } = &mut full_history_checkout.kind {
        with.insert("fetch-depth".to_owned(), "0".to_owned());
    }
    let mut deep_job = job(vec![full_history_checkout, mise_shell()]);
    crate::cache_p08::ensure_setup_p08(
        "full-history-checkout",
        &mut deep_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("full-history checkout is an eligible seed owner");
    assert!(is_tool_seed_step(&deep_job.steps[1]));

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
    crate::cache_p08::ensure_setup_p08(
        "fake-checkout",
        &mut fake_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("cold setup");
    assert!(fake_job.steps.iter().all(|step| !is_tool_seed_action(step)));

    assert_wrong_pinned_checkout_stays_cold(&setup);
}

fn assert_wrong_pinned_checkout_stays_cold(setup: &crate::MiseSetup) {
    let mut wrong_pin =
        crate::steps::checkout_step("actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect("pinned checkout shape");
    wrong_pin.name = "Checkout".to_owned();
    let mut wrong_pin_job = job(vec![wrong_pin, mise_shell()]);
    crate::cache_p08::ensure_setup_p08(
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
            .all(|step| !is_tool_seed_action(step))
    );
}

fn setup_config() -> crate::MiseSetup {
    crate::MiseSetup {
        uses: SETUP_USES.to_owned(),
        version: "2026.9.18".to_owned(),
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
        timeout_minutes: velnor_actions_contract_workflow::JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        check_runner: None,
        steps,
    }
}

#[test]
fn existing_seed_must_match_exact_job_key_and_payload() {
    let setup = setup_config();
    let mut rendered_job = job(vec![
        crate::steps::checkout_step(CHECKOUT).expect("checkout"),
        mise_shell(),
    ]);
    crate::cache_p08::ensure_setup_p08(
        "fixture",
        &mut rendered_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("setup and seed");
    let seed_index = rendered_job
        .steps
        .iter()
        .position(is_tool_seed_action)
        .expect("seed action");
    let mut mismatched_key = rendered_job.clone();
    if let StepKind::Action { with, .. } = &mut mismatched_key.steps[seed_index].kind {
        with.insert(
            "cache_key".to_owned(),
            "mise-v1-x86_64-unknown-linux-gnu-2026.9.18-0123456789abcdef".to_owned(),
        );
    }
    assert!(
        crate::cache_p08::ensure_setup_p08(
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
    let setup_index = seed_index + 1;
    if let StepKind::Action { with, .. } = &mut mismatched_setup.steps[setup_index].kind {
        with.insert(
            "cache_key".to_owned(),
            "mise-v1-x86_64-unknown-linux-gnu-2026.9.18-0123456789abcdef".to_owned(),
        );
    }
    assert!(
        crate::cache_p08::ensure_setup_p08(
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

#[test]
fn local_seed_action_payload_and_order_are_not_name_authorized() {
    let setup = setup_config();
    let mut rendered_job = job(vec![
        crate::steps::checkout_step(CHECKOUT).expect("checkout"),
        mise_shell(),
    ]);
    crate::cache_p08::ensure_setup_p08(
        "fixture",
        &mut rendered_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("setup and seed");
    let seed_index = rendered_job
        .steps
        .iter()
        .position(is_tool_seed_action)
        .expect("seed action");
    rendered_job.steps[seed_index].name = "Checkout".to_owned();
    crate::cache_p08::ensure_setup_p08(
        "renamed-seed",
        &mut rendered_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("presentation name is ignored");
    assert_eq!(rendered_job.steps[seed_index].name, "Checkout");

    let mut conditional = job(vec![
        {
            let mut step = crate::steps::checkout_step(CHECKOUT).expect("checkout");
            step.condition = Some("always()".to_owned());
            step
        },
        mise_shell(),
    ]);
    crate::cache_p08::ensure_setup_p08(
        "conditional-checkout",
        &mut conditional,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("cold setup");
    assert!(
        conditional
            .steps
            .iter()
            .all(|step| !is_tool_seed_action(step))
    );
}
