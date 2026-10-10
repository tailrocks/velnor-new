use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::*;
use crate::tool_seed_test_support::mock_trust_commands;

const TARGET: &str = "x86_64-unknown-linux-gnu";
const TARGET_RUNS: &str = "ubuntu-26.04";
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const SETUP_USES: &str = "jdx/mise-action@0123456789abcdef0123456789abcdef01234567";

fn scratch(name: &str) -> PathBuf {
    let temp_root = fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
    let path = temp_root.join(format!("velnor-tool-seed-{name}-{}", std::process::id()));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    // macOS temp lives under the /var symlink. Admission rejects that ancestor.
    fs::canonicalize(&path).expect("real scratch")
}

fn cache_key() -> String {
    tools_payload().key_expression()
}

fn runtime_key() -> String {
    format!("mise-tools-v2-{}", "a".repeat(64))
}

fn tools_payload() -> crate::cache_p08::ToolsCachePayload {
    let mise = crate::MiseSetup {
        uses: SETUP_USES.to_owned(),
        version: "2026.10.4".to_owned(),
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
    run_with_key(script, home, seed, mounts, &runtime_key())
}

fn run_with_key(script: &str, home: &Path, seed: &Path, mounts: &str, key: &str) -> Output {
    let test_dir = seed.parent().expect("seed parent");
    let script = mock_trust_commands(script, test_dir);
    let github_output = test_dir.join("GITHUB_OUTPUT");
    fs::write(&github_output, "").expect("output file");
    Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", home)
        .env("RUNNER_TEMP", home.join("runner-temp"))
        .env("RUNNER_OS", "Linux")
        .env("SEED_KEY", key)
        .env("SEED_TEST_ROOT", seed)
        .env("SEED_TEST_MOUNTS", mounts)
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .env("GITHUB_OUTPUT", &github_output)
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
    let key = runtime_key();
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
    assert_eq!(
        fs::read_to_string(root.join("GITHUB_OUTPUT")).expect("admission output"),
        "seed_admitted=true\n"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn rejected_mount_and_mismatched_key_leave_destinations_untouched() {
    let root = scratch("cold");
    let seed = root.join("seed");
    let home = root.join("home");
    ready_seed(&seed, &runtime_key());
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
    assert_eq!(
        fs::read_to_string(root.join("GITHUB_OUTPUT")).expect("rejected mount output"),
        ""
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
    assert_eq!(
        fs::read_to_string(root.join("GITHUB_OUTPUT")).expect("mismatched key output"),
        ""
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
    assert_eq!(
        fs::read_to_string(root.join("GITHUB_OUTPUT")).expect("malformed key output"),
        ""
    );
    assert!(!home.exists(), "multiline key creates no destination");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn absent_seed_does_not_report_admission() {
    let root = scratch("absent");
    let seed = root.join("seed");
    let home = root.join("home");
    let script = tool_seed_action_script(seed.to_str().expect("seed path")).expect("script");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(
        output.status.success(),
        "absent seed stays cold: {output:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("GITHUB_OUTPUT")).expect("absent seed output"),
        ""
    );
    fs::remove_dir_all(root).expect("cleanup absent seed");
}

#[test]
fn failed_second_copy_does_not_report_admission() {
    let root = scratch("copy-failure");
    let seed = root.join("seed");
    let home = root.join("home");
    ready_seed(&seed, &runtime_key());
    fs::create_dir_all(home.join("runner-temp/velnor")).expect("runner temp parent");
    fs::write(home.join("runner-temp/velnor/rustup"), "blocking file")
        .expect("blocking rustup path");
    let script = tool_seed_action_script(seed.to_str().expect("seed path")).expect("script");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(
        !output.status.success(),
        "copy failure stops workflow: {output:?}"
    );
    assert!(home.join(".local/share/mise/installs/marker").is_file());
    assert_eq!(
        fs::read_to_string(root.join("GITHUB_OUTPUT")).expect("copy output"),
        ""
    );
    fs::remove_dir_all(root).expect("cleanup copy failure");
}

#[test]
fn disabled_runtime_identity_key_cannot_import_a_matching_seed() {
    let root = scratch("disabled-runtime-identity");
    let seed = root.join("seed");
    let home = root.join("home");
    let disabled_key = "mise-tools-v2-";
    ready_seed(&seed, disabled_key);
    let script = tool_seed_action_script(seed.to_str().expect("seed path")).expect("script");
    let output = run_with_key(&script, &home, &seed, &mount(&seed), disabled_key);
    assert!(
        output.status.success(),
        "disabled identity stays cold: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("tool seed key mismatch"),
        "{output:?}"
    );
    assert!(!home.exists(), "disabled identity creates no destination");
    fs::remove_dir_all(root).expect("cleanup");
}

#[path = "tool_seed_identity_tests.rs"]
mod identity_tests;
