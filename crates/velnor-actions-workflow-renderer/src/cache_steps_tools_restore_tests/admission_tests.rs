use super::*;

#[test]
fn preserves_exact_hits_and_validated_seed_on_pure_cache_miss() {
    let expected_key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
    assert_case(
        "exact-hit",
        Some("true"),
        Some(&expected_key),
        Some(&expected_key),
        Some("false"),
        true,
    );
    assert_case(
        "seed-miss",
        None,
        Some(&expected_key),
        None,
        Some("true"),
        true,
    );
}

#[test]
fn removes_untrusted_partial_and_malformed_restore_state() {
    let expected_key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
    let other_v2_key = format!("mise-tools-v2-payload-v3-{}", "b".repeat(64));
    let previous_payload_key = format!("mise-tools-v2-{}", "a".repeat(64));
    assert_case(
        "miss-without-admitted-seed",
        None,
        Some(&expected_key),
        None,
        None,
        false,
    );
    assert_case(
        "non-exact-restore",
        Some("false"),
        Some(&expected_key),
        Some("mise-tools-v2-other"),
        Some("true"),
        false,
    );
    assert_case(
        "non-exact-v2-restore",
        Some("false"),
        Some(&expected_key),
        Some(&other_v2_key),
        Some("false"),
        false,
    );
    assert_case(
        "previous-payload-generation",
        Some("true"),
        Some(&previous_payload_key),
        Some(&previous_payload_key),
        Some("false"),
        false,
    );
    assert_case(
        "false-without-matched-key",
        Some("false"),
        Some(&expected_key),
        Some(""),
        Some("true"),
        false,
    );
    assert_case(
        "inconsistent-restore-outputs",
        Some("false"),
        Some(&expected_key),
        Some(&expected_key),
        Some("true"),
        false,
    );
    assert_case(
        "exact-hit-with-malformed-key",
        Some("true"),
        Some("mise-tools-v2-"),
        Some("mise-tools-v2-"),
        Some("true"),
        false,
    );
    assert_case(
        "missing-expected-key",
        Some("false"),
        None,
        Some(""),
        Some("true"),
        false,
    );
    assert_case(
        "malformed-expected-key",
        None,
        Some("mise-tools-v2-"),
        None,
        Some("true"),
        false,
    );
}

#[test]
fn valid_seed_without_file_payload_falls_through_to_miss_cleanup() {
    let root = scratch("empty-seed-to-miss");
    let (home, runner_temp) = populate_cache_paths(&root);
    let seed = root.join("seed");
    let key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
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
    let key = format!("mise-tools-v2-payload-v3-{}", "b".repeat(64));
    install_setup_mise(&home);
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

#[test]
fn restored_or_missing_mise_binary_must_match_the_pinned_digest() {
    let expected_key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
    for (name, hit, matched) in [
        ("tampered-hit", Some("true"), Some(expected_key.as_str())),
        ("tampered-miss", None, None),
    ] {
        let root = scratch(name);
        let (home, runner_temp) = populate_cache_paths(&root);
        fs::write(
            home.join(".local/share/mise/bin/mise"),
            "restored-other-bytes",
        )
        .expect("tampered binary");
        let output = run_script(&home, &runner_temp, hit, Some(&expected_key), matched, None);
        assert!(!output.status.success(), "{name}: {output:?}");
        fs::remove_file(home.join(".local/share/mise/bin/mise")).expect("remove binary");
        let output = run_script(&home, &runner_temp, hit, Some(&expected_key), matched, None);
        assert!(!output.status.success(), "{name} missing: {output:?}");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
