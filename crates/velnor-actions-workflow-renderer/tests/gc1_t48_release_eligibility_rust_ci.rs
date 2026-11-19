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
for arg in "$@"; do
  case "$arg" in repos/*) url="$arg" ;; esac
done
source="$VELNOR_FAKE_SOURCE"
run() {
  local id="$1" path="$2" status="$3" conclusion="$4"
  printf '[{"workflow_runs":[{"id":%s,"run_number":7,"run_attempt":1,"path":"%s","event":"push","head_branch":"main","head_sha":"%s","head_repository":{"full_name":"tailrocks/velnor-new"},"status":"%s","conclusion":%s}]}]\n' \
    "$id" "$path" "$source" "$status" "$conclusion"
}
job() {
  local id="$1" name="$2" conclusion="$3"
  printf '[{"jobs":[{"name":"%s","run_id":%s,"run_attempt":1,"head_sha":"%s","head_branch":"main","status":"completed","conclusion":"%s"}]}]\n' \
    "$name" "$id" "$source" "$conclusion"
}
case "$url" in
  repos/tailrocks/velnor-new/commits/main) printf '{"sha":"%s"}\n' "$source" ;;
  repos/tailrocks/velnor-new/actions/workflows/ci.yml/runs*)
    run 101 .github/workflows/ci.yml completed '"success"' ;;
  repos/tailrocks/velnor-new/actions/workflows/rust-ci.yml/runs*)
    case "$VELNOR_FAKE_RUST_RUN" in
      missing) printf '[{"workflow_runs":[]}]\n' ;;
      pending) run 102 .github/workflows/rust-ci.yml in_progress null ;;
      red) run 102 .github/workflows/rust-ci.yml completed '"failure"' ;;
      *) run 102 .github/workflows/rust-ci.yml completed '"success"' ;;
    esac ;;
  repos/tailrocks/velnor-new/actions/runs/101/attempts/1/jobs*) job 101 Required success ;;
  repos/tailrocks/velnor-new/actions/runs/102/attempts/1/jobs*)
    job 102 rust-required "${VELNOR_FAKE_RUST_JOB:-success}" ;;
  *) echo "unexpected request: $url" >&2; exit 3 ;;
esac
"#;

const FAKE_SLEEP: &str = "#!/bin/sh\ncase \"$1\" in 60|5) exec /bin/sleep \"$@\" ;; esac\nexit 0\n";

fn eligibility_script() -> Result<String, Box<dyn Error>> {
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
    let request = Schema2WorkflowRequest {
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        product_release: Some(pins),
        ..gc1_t48_pins::request()?
    };
    let files = render_schema2_workflows(&request)?;
    let workflow: serde_json::Value = serde_saphyr::from_str(gc1_t48_pins::find_file(
        &files,
        ".github/workflows/product-release.yml",
    )?)?;
    workflow["jobs"]["release-eligibility"]["steps"]
        .as_array()
        .ok_or_else(|| fail("eligibility steps missing"))?
        .iter()
        .find(|step| step["id"] == "check")
        .and_then(|step| step["run"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| fail("eligibility check step missing"))
}

fn write_executable(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn run_eligibility(
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
        .output()?;
    Ok((
        result.status.success(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
        std::fs::read_to_string(output_path)?,
    ))
}

#[test]
fn release_needs_a_green_rust_required_job_on_the_same_sha_beside_ci_required()
-> Result<(), Box<dyn Error>> {
    let script = eligibility_script()?;
    let (ok, stderr, output) = run_eligibility(&script, "green", "success")?;
    assert!(ok, "both green must be eligible: {stderr}");
    assert!(output.contains("ci_run_id=101"), "{output}");
    for (run, job, reason) in [
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
        ("green", "failure", "rust-required job did not succeed"),
    ] {
        let (ok, stderr, output) = run_eligibility(&script, run, job)?;
        assert!(!ok, "rust-ci {run}/{job} must not be eligible");
        assert!(stderr.contains(reason), "{run}/{job}: {stderr}");
        assert_eq!(output, "", "{run}/{job}: no outputs on failure");
    }
    Ok(())
}
