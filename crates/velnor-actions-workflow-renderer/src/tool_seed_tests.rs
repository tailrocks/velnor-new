use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use super::*;
use crate::tool_seed_test_support::mock_trust_commands;

const TARGET: &str = "x86_64-unknown-linux-gnu";
const TARGET_RUNS: &str = "ubuntu-26.04";
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const SETUP_USES: &str = "jdx/mise-action@0123456789abcdef0123456789abcdef01234567";

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("velnor-tool-seed-{name}-{}", std::process::id()));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path
}

fn cache_key() -> String {
    tools_payload().key_expression()
}

fn tools_payload() -> crate::cache_p08::ToolsCachePayload {
    let mise = crate::MiseSetup {
        uses: SETUP_USES.to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "a".repeat(64),
    };
    crate::cache_p08::ToolsCachePayload::new(crate::cache_p08::ToolsCacheInputs {
        runs_on: "ubuntu-26.04",
        target: TARGET,
        mise_setup: &mise,
        tool_specs: &["rust@1.98.1".to_owned()],
        rustup_toolchain: Some("1.98.1"),
        rustup_components: &[],
    })
    .expect("tools payload")
}

fn ready_seed(seed: &Path, key: &str) {
    fs::create_dir_all(seed.join("mise/tree/installs")).expect("mise tree");
    fs::create_dir_all(seed.join("rustup/tree/toolchains")).expect("rustup tree");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("provenance");
    fs::write(seed.join("mise/KEY"), key).expect("key file");
    fs::write(seed.join("mise/tree/installs/marker"), "mise-bytes").expect("mise marker");
    fs::write(seed.join("rustup/tree/toolchains/marker"), "rustup-bytes").expect("rustup marker");
}

fn run(script: &str, home: &Path, seed: &Path, mounts: &str) -> Output {
    let test_dir = seed.parent().expect("seed parent");
    let script = mock_trust_commands(script, test_dir);
    Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", home)
        .env("RUNNER_TEMP", home.join("runner-temp"))
        .env("RUNNER_OS", "Linux")
        .env("SEED_KEY", cache_key())
        .env("SEED_TEST_ROOT", seed)
        .env("SEED_TEST_MOUNTS", mounts)
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .output()
        .expect("run seed action")
}

fn mount(seed: &Path) -> String {
    format!("{} ext4 0:77 ro,nosuid,nodev", seed.display())
}

#[test]
fn matching_tool_seed_copies_both_trees_after_admission_and_keeps_source() {
    let root = scratch("hit");
    let seed = root.join("seed");
    let home = root.join("home");
    let key = cache_key();
    ready_seed(&seed, &key);
    let script = tool_seed_action_script(seed.to_str().expect("seed path")).expect("script");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("tool seed restored share-dir"), "{text}");
    assert!(text.contains("tool seed restored toolchain-dir"), "{text}");
    assert_eq!(
        fs::read_to_string(home.join(".local/share/mise/installs/marker")).expect("mise copy"),
        "mise-bytes"
    );
    assert_eq!(
        fs::read_to_string(home.join("runner-temp/velnor/rustup/toolchains/marker"))
            .expect("rustup copy"),
        "rustup-bytes"
    );
    assert_eq!(
        fs::read_to_string(seed.join("mise/tree/installs/marker")).expect("seed kept"),
        "mise-bytes"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn rejected_mount_and_mismatched_key_leave_destinations_untouched() {
    let root = scratch("cold");
    let seed = root.join("seed");
    let home = root.join("home");
    ready_seed(&seed, &cache_key());
    let script = tool_seed_action_script(seed.to_str().expect("seed path")).expect("script");
    let writable = format!("{} ext4 0:77 rw", seed.display());
    let output = run(&script, &home, &seed, &writable);
    assert!(
        output.status.success(),
        "untrusted seed stays cold: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("untrusted tool seed"),
        "{output:?}"
    );
    assert!(!home.exists(), "admission failure creates no destination");

    fs::write(seed.join("mise/KEY"), "mise-v1-other-key-0123456789abcdef").expect("wrong key");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(
        output.status.success(),
        "key mismatch stays cold: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("tool seed key mismatch"),
        "{output:?}"
    );
    assert!(!home.exists(), "key mismatch creates no destination");

    fs::write(seed.join("mise/KEY"), format!("{}\nextra\n", cache_key())).expect("multiline key");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(
        output.status.success(),
        "multiline key stays cold: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("tool seed key mismatch"),
        "{output:?}"
    );
    assert!(!home.exists(), "multiline key creates no destination");
    fs::remove_dir_all(root).expect("cleanup");
}

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
                    "mise-v1-x86_64-unknown-linux-gnu-2026.9.18-0123456789abcdef".to_owned(),
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
