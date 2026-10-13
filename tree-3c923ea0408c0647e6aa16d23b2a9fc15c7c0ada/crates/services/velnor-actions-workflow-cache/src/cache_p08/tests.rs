use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, JobTimeout, Step, StepKind};

use super::*;

const TARGET: &str = "x86_64-unknown-linux-gnu";
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const SETUP_USES: &str = "jdx/mise-action@0123456789abcdef0123456789abcdef01234567";

fn setup_config() -> MiseSetup {
    MiseSetup {
        uses: SETUP_USES.to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn base_job(setup: &MiseSetup) -> Job {
    let key = mise_cache_key_for_tools(
        "ubuntu26",
        TARGET,
        &setup.version,
        &["rust@1.98.1".to_owned()],
    )
    .expect("derived key");
    let setup_step = mise_setup_step_p08(setup, &key).expect("setup");
    Job {
        display_name: "Seed mutation fixture".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            velnor_actions_workflow_steps::steps::checkout_step(CHECKOUT).expect("checkout"),
            setup_step,
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
            },
        ],
    }
}

fn wrong_pin(step: &mut Step) {
    if let StepKind::Action { uses, .. } = &mut step.kind {
        *uses = "jdx/mise-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
    }
}

fn wrong_version(step: &mut Step) {
    if let StepKind::Action { with, .. } = &mut step.kind {
        with.insert("version".to_owned(), "2026.9.17".to_owned());
    }
}

fn wrong_sha(step: &mut Step) {
    if let StepKind::Action { with, .. } = &mut step.kind {
        with.insert("sha256".to_owned(), "b".repeat(64));
    }
}

fn conditional(step: &mut Step) {
    step.condition = Some("always()".to_owned());
}

fn extra_env(step: &mut Step) {
    if let StepKind::Action { env, .. } = &mut step.kind {
        env.insert("UNEXPECTED".to_owned(), "value".to_owned());
    }
}

fn wrong_key(step: &mut Step) {
    if let StepKind::Action { with, .. } = &mut step.kind {
        with.insert(
            "cache_key".to_owned(),
            "mise-v1-x86_64-unknown-linux-gnu-2026.9.18-0123456789abcdef".to_owned(),
        );
    }
}

#[test]
fn configured_setup_pin_version_sha_condition_env_and_key_are_exact() {
    let setup = setup_config();
    let mutations: [fn(&mut Step); 6] = [
        wrong_pin,
        wrong_version,
        wrong_sha,
        conditional,
        extra_env,
        wrong_key,
    ];
    for mutate in mutations {
        let mut job = base_job(&setup);
        mutate(&mut job.steps[1]);
        assert!(
            ensure_setup_p08("mutated-setup", &mut job, &setup, false, TARGET, CHECKOUT).is_err(),
            "accepted setup mutation"
        );
    }
}
