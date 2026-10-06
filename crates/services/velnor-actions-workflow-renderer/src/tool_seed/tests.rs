use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

mod keys;
mod lifecycle;

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
