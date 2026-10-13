use super::super::{
    ASSET, SOURCE_SHA_ENV, Workspace, metadata_json, run_script, scripts, successful,
};
use super::stub;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

pub(super) fn run_publish_scenario(scenario: &str) {
    let workspace = Workspace::new(scenario);
    write_assets(&workspace);
    let bin_dir = workspace.scratch.path().join("bin");
    fs::create_dir(&bin_dir).expect("stub directory");
    let gh = bin_dir.join("gh");
    fs::write(&gh, stub::gh_stub()).expect("write gh stub");
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).expect("make gh stub executable");
    let log = workspace.scratch.path().join("gh.log");
    fs::write(&log, "").expect("create command log");
    let check_count = workspace.scratch.path().join("eligibility.count");
    let poll_settings = workspace
        .scratch
        .path()
        .join("eligibility-poll-settings.log");
    fs::write(&poll_settings, "").expect("create poll settings log");
    let eligibility = eligibility_script();
    let identity = scripts::identity("cat \"$METADATA_FILE\"");
    let publish = scripts::publish(&eligibility, &identity, "aarch64-apple-darwin", ASSET);
    let output = workspace.scratch.path().join("publish.out");
    let env = publisher_environment(
        &workspace,
        scenario,
        &output,
        &log,
        &check_count,
        &poll_settings,
    );
    let result = run_script(&publish, &env, Some(&bin_dir));
    let log = fs::read_to_string(&log).expect("read command log");
    assert_publisher_result(scenario, &result, &log);
    if scenario == "success" {
        let settings = fs::read_to_string(&poll_settings).expect("read poll settings");
        assert_eq!(settings.lines().collect::<Vec<_>>(), ["1:0", "1:0"]);
    }
}

fn write_assets(workspace: &Workspace) {
    fs::write(
        &workspace.metadata,
        metadata_json(&workspace.root, &[], false),
    )
    .expect("metadata");
    let assets = workspace.root.join("assets");
    fs::create_dir(&assets).expect("assets directory");
    fs::write(assets.join(ASSET), b"compiled binary fixture\n").expect("binary asset");
    let checksum = command_output("shasum", &["-a", "256", ASSET], &assets);
    fs::write(assets.join("SHA256SUMS"), checksum).expect("checksum file");
    let receipt = format!(
        "{{\"repository\":\"example/repo-scan\",\"source_sha\":\"{}\",\"package\":\"repo-scan\",\"version\":\"0.4.3\",\"bin\":\"repo-scan\",\"target\":\"aarch64-apple-darwin\",\"tag\":\"repo-scan-v0.4.3\"}}\n",
        workspace.source_sha
    );
    fs::write(assets.join("release.json"), receipt).expect("release receipt");
}

fn publisher_environment(
    workspace: &Workspace,
    scenario: &str,
    output: &Path,
    log: &Path,
    check_count: &Path,
    poll_settings: &Path,
) -> Vec<(String, String)> {
    let mut env = workspace.env(output);
    env.extend(publisher_identity_environment(workspace, log));
    env.extend(release_expectation_environment(
        workspace,
        check_count,
        poll_settings,
    ));
    env.extend(scenario_environment(scenario));
    if scenario == "missing-token" {
        env.retain(|(key, _)| key != "IMMUTABILITY_READ_TOKEN");
    }
    env
}

fn publisher_identity_environment(workspace: &Workspace, log: &Path) -> Vec<(String, String)> {
    vec![
        (
            "RUNNER_TEMP".to_owned(),
            workspace.runner_temp.display().to_string(),
        ),
        ("GITHUB_RUN_ID".to_owned(), "17".to_owned()),
        ("GITHUB_RUN_ATTEMPT".to_owned(), "1".to_owned()),
        (
            "GITHUB_REPOSITORY".to_owned(),
            "example/repo-scan".to_owned(),
        ),
        ("GITHUB_SHA".to_owned(), workspace.source_sha.clone()),
        (SOURCE_SHA_ENV.to_owned(), workspace.source_sha.clone()),
        ("GH_LOG".to_owned(), log.display().to_string()),
    ]
}

fn release_expectation_environment(
    workspace: &Workspace,
    check_count: &Path,
    poll_settings: &Path,
) -> Vec<(String, String)> {
    vec![
        (
            "EXPECTED_AUTHORITY_SHA".to_owned(),
            workspace.source_sha.clone(),
        ),
        ("EXPECTED_CI_RUN_ID".to_owned(), "91".to_owned()),
        ("EXPECTED_CI_ATTEMPT".to_owned(), "2".to_owned()),
        ("EXPECTED_VERSION".to_owned(), "0.4.3".to_owned()),
        ("EXPECTED_TAG".to_owned(), "repo-scan-v0.4.3".to_owned()),
        ("DEFAULT_BRANCH".to_owned(), "stable".to_owned()),
        ("GH_TOKEN".to_owned(), "contents-write-token".to_owned()),
        (
            "IMMUTABILITY_READ_TOKEN".to_owned(),
            "admin-read-token".to_owned(),
        ),
        (
            "ELIGIBILITY_COUNT".to_owned(),
            check_count.display().to_string(),
        ),
        (
            "POLL_SETTINGS".to_owned(),
            poll_settings.display().to_string(),
        ),
        ("ASSET_NAME".to_owned(), ASSET.to_owned()),
    ]
}

fn scenario_environment(scenario: &str) -> Vec<(String, String)> {
    vec![
        (
            "IMMUTABLE_ENABLED".to_owned(),
            (scenario != "immutable-disabled").to_string(),
        ),
        (
            "FINAL_IMMUTABLE".to_owned(),
            (scenario != "mutable-release").to_string(),
        ),
        ("PUBLISHER_SCENARIO".to_owned(), scenario.to_owned()),
        (
            "PROTECTED_ENVIRONMENT".to_owned(),
            (scenario != "environment-unprotected" && scenario != "environment-missing")
                .to_string(),
        ),
        (
            "ENVIRONMENT_MISSING".to_owned(),
            (scenario == "environment-missing").to_string(),
        ),
        (
            "STALE_AT".to_owned(),
            if scenario == "stale-before-tag" {
                "2"
            } else if scenario == "stale-before-publish" {
                "3"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "TAG_COLLISION".to_owned(),
            (scenario == "tag-collision").to_string(),
        ),
        (
            "RELEASE_COLLISION".to_owned(),
            (scenario == "partial-collision").to_string(),
        ),
        ("NON_404".to_owned(), (scenario == "non-404").to_string()),
    ]
}

fn assert_publisher_result(scenario: &str, result: &std::process::Output, log: &str) {
    match scenario {
        "success" => assert_publisher_success(result, log),
        "immutable-disabled"
        | "environment-unprotected"
        | "environment-missing"
        | "missing-token"
        | "stale-before-tag"
        | "tag-collision"
        | "partial-collision"
        | "non-404" => assert_pre_publish_failure(scenario, result, log),
        "stale-before-publish" => assert_stale_publish_failure(result, log),
        "mutable-release"
        | "extra-release-asset"
        | "download-extra-file"
        | "download-corrupt-bytes"
        | "download-extra-checksum"
        | "final-attestation-failed" => assert_postcondition_failure(scenario, result, log),
        _ => unreachable!("known scenario"),
    }
}

fn assert_publisher_success(result: &std::process::Output, log: &str) {
    assert!(
        successful(result),
        "stdout: {}\nstderr: {}\ncommands: {log}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    for command in ["git/refs", "release create", "release edit"] {
        assert!(log.contains(command), "missing {command}: {log}");
    }
    assert!(log.contains("release view repo-scan-v0.4.3 --repo example/repo-scan --json tagName,isDraft,isImmutable,assets"));
    assert!(log.contains("release download repo-scan-v0.4.3 --repo example/repo-scan"));
    assert!(log.contains("consumer-binary-release-download"));
    assert!(log.contains("attestation verify"));
    assert_eq!(log.matches("immutable-releases").count(), 2);
    assert_eq!(
        log.matches("/environments/consumer-binary-release").count(),
        2
    );
}

fn assert_pre_publish_failure(scenario: &str, result: &std::process::Output, log: &str) {
    assert!(!successful(result), "scenario must fail closed: {scenario}");
    assert!(!log.contains("git/refs"), "tag mutation occurred: {log}");
    assert!(
        !log.contains("release create"),
        "release mutation occurred: {log}"
    );
    if matches!(scenario, "environment-unprotected" | "environment-missing") {
        assert_eq!(
            log.matches("/environments/consumer-binary-release").count(),
            1
        );
    }
}

fn assert_stale_publish_failure(result: &std::process::Output, log: &str) {
    assert!(!successful(result), "stale CI must fail before publishing");
    assert!(log.contains("git/refs"));
    assert!(log.contains("release create"));
    assert!(
        !log.contains("release edit"),
        "stale CI published draft: {log}"
    );
}

fn assert_postcondition_failure(scenario: &str, result: &std::process::Output, log: &str) {
    assert!(
        !successful(result),
        "postcondition must fail closed: {scenario}"
    );
    assert_logged_before(
        log,
        "release edit repo-scan-v0.4.3 --repo example/repo-scan --draft=false",
        "release view repo-scan-v0.4.3",
    );
    if !matches!(scenario, "mutable-release" | "extra-release-asset") {
        assert_logged_before(
            log,
            "release view repo-scan-v0.4.3",
            "release download repo-scan-v0.4.3",
        );
        assert!(
            log.contains("release download"),
            "assets not downloaded: {log}"
        );
    }
    if scenario == "download-extra-file" {
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("published asset set is unexpected"),
            "downloaded extra file did not hit the exact-set guard: {result:?}"
        );
    }
    if scenario == "final-attestation-failed" {
        assert!(
            log.contains("consumer-binary-release-download"),
            "downloaded binary not attested: {log}"
        );
    }
}

fn assert_logged_before(log: &str, before: &str, after: &str) {
    let before_index = log
        .lines()
        .position(|line| line.starts_with(before))
        .unwrap_or_else(|| panic!("missing {before}: {log}"));
    let after_index = log
        .lines()
        .position(|line| line.starts_with(after))
        .unwrap_or_else(|| panic!("missing {after}: {log}"));
    assert!(
        before_index < after_index,
        "expected {before} before {after}: {log}"
    );
}

fn eligibility_script() -> String {
    r#"release_eligibility() {
  local count=0 attempt="$EXPECTED_CI_ATTEMPT"
  if [[ -n "${VELNOR_RELEASE_CI_POLL_LIMIT+x}" ]]; then
    printf '%s:%s\n' "$VELNOR_RELEASE_CI_POLL_LIMIT" "$VELNOR_RELEASE_CI_POLL_SECONDS" >> "$POLL_SETTINGS"
  fi
  [[ ! -f "$ELIGIBILITY_COUNT" ]] || read -r count < "$ELIGIBILITY_COUNT"
  count=$((count + 1))
  printf '%s\n' "$count" > "$ELIGIBILITY_COUNT"
  [[ "${STALE_AT:-0}" != "$count" ]] || attempt=99
  printf 'source_sha=%s\nworkflow_authority_sha=%s\nci_run_id=%s\nci_attempt=%s\n' \
    "$EXPECTED_SOURCE_SHA" "$EXPECTED_AUTHORITY_SHA" "$EXPECTED_CI_RUN_ID" "$attempt" >> "$GITHUB_OUTPUT"
}
release_eligibility
"#.to_owned()
}

fn command_output(command: &str, args: &[&str], cwd: &Path) -> String {
    let output = Command::new(command)
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run helper command");
    assert!(
        output.status.success(),
        "{command} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("helper output UTF-8")
}
