use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use super::*;
use crate::tool_seed_test_support::mock_trust_commands;

const TARGET: &str = "x86_64-unknown-linux-gnu";
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const SETUP_USES: &str = "jdx/mise-action@0123456789abcdef0123456789abcdef01234567";

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("velnor-tool-seed-{name}-{}", std::process::id()));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path.canonicalize().expect("canonical scratch")
}

fn cache_key() -> String {
    crate::cache_p08::mise_cache_key_for_tools(TARGET, "2026.9.18", &["rust@1.98.1".to_owned()])
        .expect("cache key")
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

#[test]
fn action_composite_contains_the_guarded_fixed_root_and_exact_input() {
    let file = action_file("0.1.0").expect("action");
    assert_eq!(file.path, TOOL_SEED_ACTION_PATH);
    assert!(file.bytes.contains("/opt/velnor/seed"), "{}", file.bytes);
    assert!(file.bytes.contains("$SEED_KEY"), "{}", file.bytes);
    assert!(file.bytes.contains("inputs.cache_key"), "{}", file.bytes);
    assert!(
        file.bytes.contains("trusted_seed_is_trusted"),
        "{}",
        file.bytes
    );
    assert!(file.bytes.contains("unset "), "{}", file.bytes);
    assert!(!file.bytes.contains("rm "), "{}", file.bytes);
}
