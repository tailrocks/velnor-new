// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    ffi::OsString,
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use super::{seed_policy, workflow_run_event};

pub(crate) const REPOSITORY: &str = "test-owner/test-tap";
pub(crate) const REPOSITORY_ID: u64 = 819_273;
pub(crate) const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) const AUTHORITY_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(crate) const PROTECTED_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const WRONG_SHA: &str = "dddddddddddddddddddddddddddddddddddddddd";
const ROOT_TREE_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const MANIFEST_BLOB_SHA: &str = "ffffffffffffffffffffffffffffffffffffffff";
const MAIN_BLOB_SHA: &str = "0000000000000000000000000000000000000000";
pub(crate) const CI_RUN_ID: u64 = 100;
const PREVIEW_RUN_ID: u64 = 200;
const PREVIEW_WORKFLOW_PATH: &str = ".github/workflows/binary-preview.yml";

pub(crate) type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub(crate) struct Invocation {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) output: String,
    pub(crate) requests: String,
    pub(crate) gh_calls: String,
}

struct PreparedFixture {
    runner_temp: PathBuf,
    home: PathBuf,
    mise_data_dir: PathBuf,
    path: OsString,
    event_path: PathBuf,
    github_output: PathBuf,
    policy_path: PathBuf,
    policy_text: String,
    request_log: PathBuf,
    gh_call_log: PathBuf,
}

pub(crate) fn invoke(scenario: &str) -> TestResult<Invocation> {
    let temp = tempfile::tempdir()?;
    let fixture = prepare_fixture(temp.path(), scenario)?;
    run_cli(&fixture)
}

fn prepare_fixture(root: &Path, scenario: &str) -> TestResult<PreparedFixture> {
    let runner_temp = root.join("runner");
    let policy_dir = runner_temp.join("velnor");
    let home = root.join("home");
    let mise_data_dir = root.join("mise-data");
    let archive = match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => "macOS_arm64",
        ("macos", "x86_64") => "macOS_amd64",
        ("linux", "aarch64") => "linux_arm64",
        ("linux", "x86_64") => "linux_amd64",
        (os, arch) => {
            return Err(
                io::Error::other(format!("unsupported GH fixture host {os}/{arch}")).into(),
            );
        }
    };
    let gh_install = mise_data_dir
        .join("installs")
        .join("gh")
        .join(velnor_actions_mise::GH_VERSION)
        .join(format!("gh_{}_{archive}", velnor_actions_mise::GH_VERSION))
        .join("bin");
    fs::create_dir_all(&policy_dir)?;
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&gh_install)?;

    let policy = seed_policy().canonical_bytes()?;
    let policy_path = policy_dir.join("preview-source-seed-policy.json");
    fs::write(&policy_path, &policy)?;
    let event_path = root.join("workflow-run-event.json");
    fs::write(&event_path, serde_json::to_vec(&workflow_run_event())?)?;
    let github_output = root.join("github-output");
    fs::write(&github_output, b"")?;
    let request_log = mise_data_dir.join("gh-request-media.tsv");
    fs::write(&request_log, b"")?;
    let gh_call_log = mise_data_dir.join("gh-invocations.log");
    fs::write(&gh_call_log, b"")?;

    let oversize_json = format!(
        "{{\"sha\":\"{SOURCE_SHA}\",\"padding\":\"{}\"}}\n",
        "x".repeat(1_048_700)
    );
    assert!(oversize_json.len() > 1024 * 1024);
    let oversize_path = mise_data_dir.join("oversized-commit.json");
    fs::write(&oversize_path, oversize_json.as_bytes())?;

    let gh = gh_install.join("gh");
    let gh_fixture = gh_fixture(scenario);
    fs::write(&gh, gh_fixture)?;
    let mut permissions = fs::metadata(&gh)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gh, permissions)?;

    let path = env::var_os("PATH").ok_or_else(|| io::Error::other("test PATH is missing"))?;
    verify_real_mise(&path, &home, &mise_data_dir, &gh, root)?;
    Ok(PreparedFixture {
        runner_temp,
        home,
        mise_data_dir,
        path,
        event_path,
        github_output,
        policy_path,
        policy_text: String::from_utf8(policy)?,
        request_log,
        gh_call_log,
    })
}

fn gh_fixture(scenario: &str) -> String {
    GH_FIXTURE
        .replace("@GH_VERSION@", velnor_actions_mise::GH_VERSION)
        .replace("@SCENARIO@", scenario)
        .replace("@SOURCE_SHA@", SOURCE_SHA)
        .replace("@AUTHORITY_SHA@", AUTHORITY_SHA)
        .replace("@PROTECTED_SHA@", PROTECTED_SHA)
        .replace("@WRONG_SHA@", WRONG_SHA)
        .replace("@ROOT_TREE_SHA@", ROOT_TREE_SHA)
        .replace("@MANIFEST_BLOB_SHA@", MANIFEST_BLOB_SHA)
        .replace("@MAIN_BLOB_SHA@", MAIN_BLOB_SHA)
}

fn verify_real_mise(
    path: &OsString,
    home: &Path,
    mise_data_dir: &Path,
    expected_gh: &Path,
    root: &Path,
) -> TestResult<()> {
    let real_mise = find_on_path("mise", path)
        .ok_or_else(|| io::Error::other("the caller's real Mise executable is unavailable"))?;
    assert!(
        !real_mise.starts_with(root),
        "the test must use the caller's real Mise, not a fixture executable"
    );
    let version = Command::new(&real_mise)
        .arg("--version")
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("MISE_DATA_DIR", mise_data_dir)
        .env("MISE_NO_CONFIG", "1")
        .env("MISE_NO_ENV", "1")
        .env("MISE_NO_HOOKS", "1")
        .output()?;
    assert!(
        version.status.success(),
        "real Mise version check failed: {}",
        String::from_utf8_lossy(&version.stderr)
    );
    let resolution = Command::new(real_mise)
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            &format!("gh@{}", velnor_actions_mise::GH_VERSION),
            "--",
            "/bin/sh",
            "-c",
            "command -v gh",
        ])
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("MISE_DATA_DIR", mise_data_dir)
        .env("MISE_NO_CONFIG", "1")
        .env("MISE_NO_ENV", "1")
        .env("MISE_NO_HOOKS", "1")
        .env("MISE_LOCKFILE", "0")
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .output()?;
    assert!(
        resolution.status.success(),
        "real Mise could not resolve pinned GH: {}",
        String::from_utf8_lossy(&resolution.stderr)
    );
    let resolved_gh = PathBuf::from(String::from_utf8(resolution.stdout)?.trim());
    assert_eq!(
        resolved_gh.canonicalize()?,
        expected_gh.canonicalize()?,
        "real Mise must select the private pinned GH fixture"
    );
    Ok(())
}

fn run_cli(fixture: &PreparedFixture) -> TestResult<Invocation> {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .env_clear()
        .env("PATH", &fixture.path)
        .env("HOME", &fixture.home)
        .env("MISE_DATA_DIR", &fixture.mise_data_dir)
        .env("MISE_NO_CONFIG", "1")
        .env("MISE_NO_ENV", "1")
        .env("MISE_NO_HOOKS", "1")
        .env("MISE_LOCKFILE", "0")
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .env("VELNOR_INTERNAL_OP", "select-preview-source-v1")
        .env("RUNNER_TEMP", &fixture.runner_temp)
        .env("VELNOR_REQUEST_FILE", &fixture.policy_path)
        .env("VELNOR_RELEASE_POLICY", &fixture.policy_text)
        .env("VELNOR_RELEASE_STATUS_TOKEN", "fixture-status-read-token")
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_REPOSITORY_ID", REPOSITORY_ID.to_string())
        .env("GITHUB_SHA", AUTHORITY_SHA)
        .env("GITHUB_WORKFLOW_SHA", AUTHORITY_SHA)
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            format!("{REPOSITORY}/{PREVIEW_WORKFLOW_PATH}@refs/heads/main"),
        )
        .env("GITHUB_EVENT_NAME", "workflow_run")
        .env("GITHUB_EVENT_PATH", &fixture.event_path)
        .env("GITHUB_RUN_ID", PREVIEW_RUN_ID.to_string())
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_JOB", "preview-eligibility")
        .env("GITHUB_OUTPUT", &fixture.github_output)
        .output()?;
    let requests = fs::read_to_string(&fixture.request_log).unwrap_or_default();
    let gh_calls = fs::read_to_string(&fixture.gh_call_log).unwrap_or_default();
    Ok(Invocation {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
        output: fs::read_to_string(&fixture.github_output)?,
        requests,
        gh_calls,
    })
}

fn find_on_path(program: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    env::split_paths(path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
}

const GH_FIXTURE: &str = r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$MISE_DATA_DIR/gh-invocations.log"
if [ "$1" = "--version" ]; then
  printf '%s\n' 'gh version @GH_VERSION@ (fixture transport)'
  exit 0
fi
[ "$1" = "api" ] || exit 91
shift
accept=
route=
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-H" ]; then
    shift
    case "$1" in
      Accept:*) accept=$(printf '%s\n' "$1" | cut -d ' ' -f2-) ;;
    esac
  else
    route="$1"
  fi
  shift
done
printf '%s\t%s\n' "$accept" "$route" >> "$MISE_DATA_DIR/gh-request-media.tsv"
base=$(printf '%s\n' "$route" | cut -d '?' -f 1)
scenario='@SCENARIO@'; source_sha='@SOURCE_SHA@'; authority_sha='@AUTHORITY_SHA@'; protected_sha='@PROTECTED_SHA@'
wrong_sha='@WRONG_SHA@'; root_tree_sha='@ROOT_TREE_SHA@'; manifest_blob_sha='@MANIFEST_BLOB_SHA@'; main_blob_sha='@MAIN_BLOB_SHA@'
ci_run='{"id":100,"run_attempt":1,"run_number":100,"workflow_id":10,"name":"CI","path":".github/workflows/ci.yml","head_sha":"'"$source_sha"'","head_branch":"main","event":"push","status":"completed","conclusion":"success","repository":{"id":819273,"full_name":"test-owner/test-tap"},"head_repository":{"id":819273,"full_name":"test-owner/test-tap"}}'
preview_run='{"id":200,"run_attempt":1,"run_number":200,"workflow_id":20,"name":"Rust binary preview","path":".github/workflows/binary-preview.yml","head_sha":"'"$authority_sha"'","head_branch":"main","event":"workflow_run","status":"in_progress","repository":{"id":819273,"full_name":"test-owner/test-tap"},"head_repository":{"id":819273,"full_name":"test-owner/test-tap"}}'
case "$base" in
  repos/test-owner/test-tap) printf '%s\n' '{"id":819273,"full_name":"test-owner/test-tap","default_branch":"main"}' ;;
  repos/test-owner/test-tap/rules/branches/main) printf '%s\n' '[{"type":"pull_request","parameters":{"required_approving_review_count":1,"required_review_thread_resolution":true}},{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Required"}]}},{"type":"non_fast_forward"},{"type":"deletion"}]' ;;
  repos/test-owner/test-tap/branches/main/protection)
    printf '%s\n' '{"status":"404","message":"Not Found"}'; printf '%s\n' '(HTTP 404)' >&2; exit 1
    ;;
  repos/test-owner/test-tap/environments/preview-publish) printf '%s\n' '{"name":"preview-publish","deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true},"protection_rules":[{"type":"branch_policy"}]}' ;;
  repos/test-owner/test-tap/environments/preview-publish/deployment-branch-policies*) printf '%s\n' '{"total_count":1,"branch_policies":[{"name":"main"}]}' ;;
  repos/test-owner/test-tap/branches/main) printf '{"name":"main","protected":true,"commit":{"sha":"%s"}}\n' "$protected_sha" ;;
  repos/test-owner/test-tap/commits/*)
    commit_sha=$(printf '%s\n' "$base" | awk -F/ '{print $NF}')
    reported_sha="$commit_sha"
    if [ "$scenario" = "wrong-sha" ] && [ "$commit_sha" = "$source_sha" ]; then reported_sha="$wrong_sha"; fi
    if [ "$accept" = "application/vnd.github.sha" ]; then
      printf '%s\n' "$reported_sha"
    elif [ "$scenario" = "large-json" ]; then
      cat "$MISE_DATA_DIR/oversized-commit.json"
    else
      printf '{"sha":"%s"}\n' "$reported_sha"
    fi
    ;;
  repos/test-owner/test-tap/compare/*)
    if [ "$base" = "repos/test-owner/test-tap/compare/$source_sha...$authority_sha" ]; then
      printf '{"base_commit":{"sha":"%s"},"merge_base_commit":{"sha":"%s"},"status":"ahead","ahead_by":1,"total_commits":1}\n' "$source_sha" "$source_sha"
    elif [ "$base" = "repos/test-owner/test-tap/compare/$authority_sha...$protected_sha" ]; then
      printf '{"base_commit":{"sha":"%s"},"merge_base_commit":{"sha":"%s"},"status":"ahead","ahead_by":1,"total_commits":1}\n' "$authority_sha" "$authority_sha"
    else
      exit 92
    fi
    ;;
  repos/test-owner/test-tap/git/commits/*)
    commit_sha=$(printf '%s\n' "$base" | awk -F/ '{print $NF}')
    if [ "$commit_sha" = "$source_sha" ]; then
      printf '{"sha":"%s","tree":{"sha":"%s"},"parents":[]}\n' "$source_sha" "$root_tree_sha"
    elif [ "$commit_sha" = "$authority_sha" ]; then
      printf '{"sha":"%s","parents":[{"sha":"%s"}]}\n' "$authority_sha" "$source_sha"
    elif [ "$commit_sha" = "$protected_sha" ]; then
      printf '{"sha":"%s","parents":[{"sha":"%s"}]}\n' "$protected_sha" "$authority_sha"
    else
      exit 93
    fi
    ;;
  repos/test-owner/test-tap/git/trees/*)
    printf '{"sha":"%s","truncated":false,"tree":[{"path":"Cargo.toml","mode":"100644","type":"blob","sha":"%s"},{"path":"src/main.rs","mode":"100644","type":"blob","sha":"%s"}]}\n' "$root_tree_sha" "$manifest_blob_sha" "$main_blob_sha"
    ;;
  repos/test-owner/test-tap/actions/workflows/ci.yml) printf '%s\n' '{"id":10,"name":"CI","path":".github/workflows/ci.yml","state":"active"}' ;;
  repos/test-owner/test-tap/actions/workflows/binary-preview.yml) printf '%s\n' '{"id":20,"name":"Rust binary preview","path":".github/workflows/binary-preview.yml","state":"active"}' ;;
  repos/test-owner/test-tap/actions/runs/100/attempts/1|repos/test-owner/test-tap/actions/runs/100) printf '%s\n' "$ci_run" ;;
  repos/test-owner/test-tap/actions/runs/100/attempts/1/jobs*) printf '{"total_count":1,"jobs":[{"id":101,"run_id":100,"name":"Required","head_sha":"%s","status":"completed","conclusion":"success"}]}\n' "$source_sha" ;;
  repos/test-owner/test-tap/actions/workflows/10/runs*) printf '{"workflow_runs":[%s]}\n' "$ci_run" ;;
  repos/test-owner/test-tap/actions/runs/200/attempts/1|repos/test-owner/test-tap/actions/runs/200) printf '%s\n' "$preview_run" ;;
  repos/test-owner/test-tap/actions/runs/200/attempts/1/jobs*) printf '{"total_count":1,"jobs":[{"id":201,"run_id":200,"name":"Select authenticated main source for preview","head_sha":"%s","status":"in_progress","started_at":"2026-10-27T00:00:00Z"}]}\n' "$authority_sha" ;;
  *)
    printf 'unexpected fixture route: %s\n' "$base" >&2; exit 94
    ;;
esac
"#;
