//! Seed-import handoff into the restore admission on a cache miss.

use std::fs;
use std::process::Command;

use super::{assert_paths_removed, install_setup_mise, populate_cache_paths, run_script, scratch};

#[test]
fn valid_seed_without_file_payload_falls_through_to_miss_cleanup() {
    let root = scratch("empty-seed-to-miss");
    let (home, runner_temp) = populate_cache_paths(&root);
    let seed = root.join("seed");
    let key = format!("mise-tools-v2-{}", "a".repeat(64));
    fs::create_dir_all(seed.join("mise/tree")).expect("empty mise tree");
    fs::create_dir_all(seed.join("rustup/tree")).expect("empty rustup tree");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("provenance");
    fs::write(seed.join("mise/KEY"), &key).expect("seed key");
    let github_output = root.join("GITHUB_OUTPUT");
    fs::write(&github_output, "").expect("composite output");

    let script = crate::tool_seed::tool_seed_action_script(seed.to_str().expect("seed path"))
        .expect("seed script");
    let script = crate::tool_seed_test_support::mock_trust_commands(&script, &root);
    let seed_run = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", &home)
        .env("RUNNER_TEMP", &runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("SEED_KEY", &key)
        .env("SEED_TEST_ROOT", &seed)
        .env(
            "SEED_TEST_MOUNTS",
            format!("{} ext4 0:77 ro,nosuid,nodev", seed.display()),
        )
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .env("GITHUB_OUTPUT", &github_output)
        .output()
        .expect("run seed import");
    assert!(seed_run.status.success(), "{seed_run:?}");
    let admitted_output = fs::read_to_string(&github_output).expect("seed output");
    let admitted = admitted_output
        .strip_prefix("seed_admitted=")
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let restore = run_script(&home, &runner_temp, None, Some(&key), None, admitted);
    assert!(restore.status.success(), "{restore:?}");
    assert_paths_removed(&home, &runner_temp);
    assert_eq!(admitted_output, "");
    fs::remove_dir_all(root).expect("cleanup empty-seed miss");
}

#[test]
fn one_tree_seed_admission_preserves_only_the_imported_payload_on_miss() {
    let root = fs::canonicalize(scratch("single-tree-seed-miss")).expect("real scratch");
    let home = root.join("home");
    let runner_temp = root.join("runner-temp");
    let seed = root.join("seed");
    let key = format!("mise-tools-v2-{}", "b".repeat(64));
    let mise_marker = seed.join("mise/tree/installs/marker");
    fs::create_dir_all(mise_marker.parent().expect("Mise tree parent")).expect("Mise seed tree");
    fs::write(&mise_marker, "mise-seed-bytes").expect("Mise seed payload");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("provenance");
    fs::write(seed.join("mise/KEY"), &key).expect("seed key");
    let github_output = root.join("GITHUB_OUTPUT");
    fs::write(&github_output, "").expect("composite output");

    let script = crate::tool_seed::tool_seed_action_script(seed.to_str().expect("seed path"))
        .expect("seed script");
    let script = crate::tool_seed_test_support::mock_trust_commands(&script, &root);
    let seed_run = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", &home)
        .env("RUNNER_TEMP", &runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("SEED_KEY", &key)
        .env("SEED_TEST_ROOT", &seed)
        .env(
            "SEED_TEST_MOUNTS",
            format!("{} ext4 0:77 ro,nosuid,nodev", seed.display()),
        )
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .env("GITHUB_OUTPUT", &github_output)
        .output()
        .expect("run one-tree seed import");
    assert!(seed_run.status.success(), "{seed_run:?}");
    let admitted_output = fs::read_to_string(&github_output).expect("seed output");
    assert_eq!(admitted_output, "seed_admitted=true\n", "{seed_run:?}");
    let admitted = admitted_output
        .strip_prefix("seed_admitted=")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let imported_mise = home.join(".local/share/mise/installs/marker");
    assert_eq!(
        fs::read_to_string(&imported_mise).expect("Mise import"),
        "mise-seed-bytes"
    );
    assert!(!runner_temp.join("velnor/rustup").exists());
    install_setup_mise(&home);

    let restore = run_script(&home, &runner_temp, None, Some(&key), None, admitted);
    assert!(restore.status.success(), "{restore:?}");
    assert_eq!(
        fs::read_to_string(imported_mise).expect("retained Mise import"),
        "mise-seed-bytes"
    );
    assert!(!runner_temp.join("velnor/rustup").exists());
    assert!(!runner_temp.join("velnor/cargo/bin").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates.toml").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates2.json").exists());
    fs::remove_dir_all(root).expect("cleanup one-tree miss");
}
