// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::os::unix::fs::PermissionsExt;

const REPOSITORY: &str = "tailrocks/velnor-new";

const FAKE_MISE: &str = r#"#!/usr/bin/env bash
while [[ $# -gt 0 && "$1" != "--" ]]; do shift; done
shift
shift
[[ "$1" == api ]] || exit 2
url=""
raw=""
for arg in "$@"; do
  case "$arg" in repos/*) url="$arg" ;; --jq) raw=1 ;; esac
done
source="$VELNOR_FAKE_SOURCE"
entry() {
  printf '{"id":%s,"run_number":%s,"run_attempt":1,"path":"%s","event":"push","head_branch":"main","head_sha":"%s","head_repository":{"full_name":"tailrocks/velnor-new"},"status":"%s","conclusion":%s}' \
    "$1" "$2" "$3" "$source" "$4" "$5"
}
page() { printf '[{"workflow_runs":[%s]}]\n' "$1"; }
jobs() {
  local id="$1" name="$2" conclusion="$3"
  printf '{"name":"%s","run_id":%s,"run_attempt":1,"head_sha":"%s","head_branch":"main","status":"completed","conclusion":"%s"}' \
    "$name" "$id" "$source" "$conclusion"
}
rust=.github/workflows/rust-ci.yml
case "$url" in
  repos/tailrocks/velnor-new/commits/main)
    if [[ -n "$raw" ]]; then printf '%s\n' "$source"; else printf '{"sha":"%s"}\n' "$source"; fi ;;
  repos/tailrocks/velnor-new/actions/workflows/ci.yml/runs*)
    page "$(entry 101 7 .github/workflows/ci.yml completed '"success"')" ;;
  repos/tailrocks/velnor-new/actions/workflows/rust-ci.yml/runs*)
    case "$VELNOR_FAKE_RUST_RUN" in
      missing) page "" ;;
      pending) page "$(entry 102 7 $rust in_progress null)" ;;
      red) page "$(entry 102 7 $rust completed '"failure"')" ;;
      cancelled) page "$(entry 102 7 $rust completed '"cancelled"')" ;;
      tie) page "$(entry 102 7 $rust completed '"success"'),$(entry 103 7 $rust completed '"success"')" ;;
      changed)
        reads=$(( $(cat "$VELNOR_FAKE_STATE" 2>/dev/null || echo 0) + 1 ))
        printf '%s' "$reads" > "$VELNOR_FAKE_STATE"
        if (( reads == 1 )); then page "$(entry 102 7 $rust completed '"success"')"
        else page "$(entry 103 8 $rust completed '"success"')"; fi ;;
      *) page "$(entry 102 7 $rust completed '"success"')" ;;
    esac ;;
  repos/tailrocks/velnor-new/actions/runs/101/attempts/1/jobs*)
    printf '[{"jobs":[%s]}]\n' "$(jobs 101 Required success)" ;;
  repos/tailrocks/velnor-new/actions/runs/102/attempts/1/jobs*)
    case "$VELNOR_FAKE_RUST_JOB" in
      duplicate) printf '[{"jobs":[%s,%s]}]\n' "$(jobs 102 rust-required success)" "$(jobs 102 rust-required success)" ;;
      absent) printf '[{"jobs":[%s]}]\n' "$(jobs 102 rust-lint success)" ;;
      *) printf '[{"jobs":[%s]}]\n' "$(jobs 102 rust-required "${VELNOR_FAKE_RUST_JOB:-success}")" ;;
    esac ;;
  *) echo "unexpected request: $url" >&2; exit 3 ;;
esac
"#;

const FAKE_SLEEP: &str = "#!/bin/sh\ncase \"$1\" in 60|5) exec /bin/sleep \"$@\" ;; esac\nexit 0\n";

pub(crate) fn gate_pins() -> ProductReleasePins {
    let mut pins = gc1_t48_pins::pins();
    pins.gh_argv = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "gh@2.103.3",
        "--",
        "gh",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    pins
}

pub(crate) fn rendered_workflow(path: &str) -> Result<serde_json::Value, Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        product_release: Some(gate_pins()),
        ..gc1_t48_pins::request()?
    };
    let files = render_schema2_workflows(&request)?;
    Ok(serde_saphyr::from_str(gc1_t48_pins::find_file(
        &files, path,
    )?)?)
}

fn write_executable(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(crate) fn run_gate_script(
    script: &str,
    rust_run: &str,
    rust_job: &str,
) -> Result<(bool, String, String), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let bin = temp.path().join("bin");
    std::fs::create_dir(&bin)?;
    write_executable(&bin.join("mise"), FAKE_MISE)?;
    write_executable(&bin.join("sleep"), FAKE_SLEEP)?;
    let work = temp.path().join("work");
    std::fs::create_dir(&work)?;
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec![
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(&args)
                .current_dir(&work)
                .status()?
                .success()
        );
    }
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&work)
            .output()?
            .stdout,
    )?
    .trim()
    .to_owned();
    let output_path = temp.path().join("output");
    std::fs::write(&output_path, "")?;
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let result = Command::new("bash")
        .args(["-c", script])
        .current_dir(&work)
        .env("PATH", path)
        .env("GH_TOKEN", "token")
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            format!("{REPOSITORY}/.github/workflows/product-release.yml@refs/heads/main"),
        )
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_SHA", &head)
        .env("GITHUB_WORKFLOW_SHA", &head)
        .env("GITHUB_OUTPUT", &output_path)
        .env("VELNOR_FAKE_SOURCE", &head)
        .env("VELNOR_FAKE_RUST_RUN", rust_run)
        .env("VELNOR_FAKE_RUST_JOB", rust_job)
        .env("VELNOR_FAKE_STATE", temp.path().join("state"))
        .output()?;
    Ok((
        result.status.success(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
        std::fs::read_to_string(output_path)?,
    ))
}

pub(crate) const FAIL_CLOSED_CASES: [(&str, &str, &str); 9] = [
    (
        "missing",
        "success",
        "latest exact-source Rust CI run is missing",
    ),
    (
        "pending",
        "success",
        "latest exact-source Rust CI run is not complete",
    ),
    (
        "red",
        "success",
        "latest exact-source Rust CI run did not succeed",
    ),
    (
        "cancelled",
        "success",
        "latest exact-source Rust CI run did not succeed",
    ),
    ("tie", "success", "Rust CI run response is invalid"),
    ("green", "failure", "rust-required job did not succeed"),
    (
        "green",
        "duplicate",
        "rust-required job response is invalid",
    ),
    ("green", "absent", "rust-required job response is invalid"),
    (
        "changed",
        "success",
        "latest Rust CI run changed during eligibility check",
    ),
];
