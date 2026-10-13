use super::{ASSET, SOURCE_SHA_ENV, Workspace, metadata_json, run_script, scripts, successful};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

#[test]
fn publisher_gates_mutation_on_immutable_setting_current_eligibility_and_absence() {
    for scenario in [
        "success",
        "immutable-disabled",
        "environment-unprotected",
        "environment-missing",
        "missing-token",
        "stale-before-tag",
        "stale-before-publish",
        "tag-collision",
        "partial-collision",
        "non-404",
    ] {
        run_publish_scenario(scenario);
    }
}

fn run_publish_scenario(scenario: &str) {
    let workspace = Workspace::new(scenario);
    write_assets(&workspace);
    let bin_dir = workspace.scratch.path().join("bin");
    fs::create_dir(&bin_dir).expect("stub directory");
    let gh = bin_dir.join("gh");
    fs::write(&gh, gh_stub()).expect("write gh stub");
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).expect("make gh stub executable");
    let log = workspace.scratch.path().join("gh.log");
    fs::write(&log, "").expect("create command log");
    let check_count = workspace.scratch.path().join("eligibility.count");
    let eligibility = eligibility_script();
    let identity = scripts::identity("cat \"$METADATA_FILE\"");
    let publish = scripts::publish(&eligibility, &identity, "aarch64-apple-darwin", ASSET);
    let output = workspace.scratch.path().join("publish.out");
    let env = publisher_environment(&workspace, scenario, &output, &log, &check_count);
    let result = run_script(&publish, &env, Some(&bin_dir));
    let log = fs::read_to_string(&log).expect("read command log");
    assert_publisher_result(scenario, &result, &log);
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
) -> Vec<(String, String)> {
    let mut env = workspace.env(output);
    env.extend([
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
        ("GH_LOG".to_owned(), log.display().to_string()),
        (
            "ELIGIBILITY_COUNT".to_owned(),
            check_count.display().to_string(),
        ),
        (
            "IMMUTABLE_ENABLED".to_owned(),
            (scenario != "immutable-disabled").to_string(),
        ),
        ("FINAL_IMMUTABLE".to_owned(), "true".to_owned()),
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
        ("ASSET_NAME".to_owned(), ASSET.to_owned()),
    ]);
    if scenario == "missing-token" {
        env.retain(|(key, _)| key != "IMMUTABILITY_READ_TOKEN");
    }
    env
}

fn assert_publisher_result(scenario: &str, result: &std::process::Output, log: &str) {
    match scenario {
        "success" => {
            assert!(
                successful(result),
                "stdout: {}\nstderr: {}\ncommands: {log}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr),
            );
            assert!(log.contains("git/refs"));
            assert!(log.contains("release create"));
            assert!(log.contains("release edit"));
            assert_eq!(log.matches("immutable-releases").count(), 2);
            assert_eq!(
                log.matches("/environments/consumer-binary-release").count(),
                2
            );
        }
        "immutable-disabled"
        | "environment-unprotected"
        | "environment-missing"
        | "missing-token"
        | "stale-before-tag"
        | "tag-collision"
        | "partial-collision"
        | "non-404" => {
            assert!(!successful(result), "scenario must fail closed: {scenario}");
            assert!(
                !log.contains("git/refs"),
                "no tag mutation for {scenario}: {log}"
            );
            assert!(
                !log.contains("release create"),
                "no release mutation for {scenario}: {log}"
            );
            if scenario == "environment-unprotected" || scenario == "environment-missing" {
                assert_eq!(
                    log.matches("/environments/consumer-binary-release").count(),
                    1
                );
            }
        }
        "stale-before-publish" => {
            assert!(!successful(result), "stale CI must fail before publishing");
            assert!(log.contains("git/refs"));
            assert!(log.contains("release create"));
            assert!(
                !log.contains("release edit"),
                "stale CI must not publish draft: {log}"
            );
        }
        _ => unreachable!("known scenario"),
    }
}

fn eligibility_script() -> String {
    r#"release_eligibility() {
  local count=0 attempt="$EXPECTED_CI_ATTEMPT"
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

fn gh_stub() -> &'static str {
    r#"#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_LOG"
case "$1" in
  attestation)
    if [[ "$2" == download ]]; then
      digest="$(shasum -a 256 "$3" | awk '{print $1}')"
      printf '{"fixture":true}\n' > "sha256:${digest}.jsonl"
    fi
    exit 0
    ;;
  api)
    endpoint=""
    for arg in "$@"; do [[ "$arg" != repos/* ]] || endpoint="$arg"; done
    case "$endpoint" in
      */immutable-releases)
        [[ "$GH_TOKEN" == "$IMMUTABILITY_READ_TOKEN" ]]
        [[ "$IMMUTABLE_ENABLED" == true ]] && printf '{"enabled":true,"enforced_by_owner":false}\n' || printf '{"enabled":false,"enforced_by_owner":false}\n'
        ;;
      */environments/consumer-binary-release)
        if [[ "$ENVIRONMENT_MISSING" == true ]]; then
          echo 'Not Found (HTTP 404)' >&2
          exit 1
        fi
        if [[ "$PROTECTED_ENVIRONMENT" == true ]]; then
          printf '{"name":"consumer-binary-release","protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"login":"reviewer"}}],"prevent_self_review":true}],"deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false}}\n'
        else
          printf '{"name":"consumer-binary-release","protection_rules":[],"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":false}}\n'
        fi
        ;;
      */git/ref/tags/*)
        if [[ -f "$GH_LOG.tag-created" || "$TAG_COLLISION" == true ]]; then
          printf '{"ref":"refs/tags/%s","object":{"sha":"%s"}}\n' "$EXPECTED_TAG" "$EXPECTED_SOURCE_SHA"
        elif [[ "$NON_404" == true ]]; then
          echo 'Forbidden (HTTP 403)' >&2
          exit 1
        else
          echo 'Not Found (HTTP 404)' >&2
          exit 1
        fi
        ;;
      */releases/tags/*)
        if [[ "$RELEASE_COLLISION" == true ]]; then
          printf '{"tag_name":"%s"}\n' "$EXPECTED_TAG"
        elif [[ "$NON_404" == true ]]; then
          echo 'Forbidden (HTTP 403)' >&2
          exit 1
        else
          echo 'Not Found (HTTP 404)' >&2
          exit 1
        fi
        ;;
      */git/refs)
        : > "$GH_LOG.tag-created"
        printf '{"ref":"refs/tags/%s","object":{"sha":"%s"}}\n' "$EXPECTED_TAG" "$EXPECTED_SOURCE_SHA"
        ;;
      *) echo "unexpected API endpoint: $endpoint" >&2; exit 1 ;;
    esac
    ;;
  release)
    case "$2" in
      create) ;;
      edit) ;;
      view)
        printf '{"tagName":"%s","isDraft":false,"isImmutable":%s,"assets":[{"name":"%s"},{"name":"SHA256SUMS"},{"name":"release.json"}]}\n' \
          "$EXPECTED_TAG" "$FINAL_IMMUTABLE" "$ASSET_NAME"
        ;;
      download)
        destination=""
        while (($#)); do
          if [[ "$1" == --dir ]]; then destination="$2"; shift 2; else shift; fi
        done
        mkdir -p "$destination"
        cp "$ASSET_NAME" SHA256SUMS release.json "$destination/"
        ;;
      *) echo "unexpected release command: $2" >&2; exit 1 ;;
    esac
    ;;
  *) echo "unexpected gh command: $1" >&2; exit 1 ;;
esac
"#
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
