//! Executed fixture tests for controller identity and result classification.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{render, scripts};
use crate::yaml::Yaml;
use velnor_actions_contract::StepKind;

#[path = "schema2_mbx_cancel_probe_classification_tests.rs"]
mod classifications;
#[path = "schema2_mbx_cancel_probe_controller_fixture_tests.rs"]
mod controller_fixtures;
#[path = "schema2_mbx_cancel_probe_cache_snapshot_tests.rs"]
mod cache_snapshots;
#[path = "schema2_mbx_cancel_probe_controller_transport_cases.rs"]
mod controller_transport;
#[path = "schema2_mbx_cancel_probe_key_fixture_tests.rs"]
mod key_fixtures;
#[path = "schema2_mbx_cancel_probe_native_render_tests.rs"]
mod native_render_tests;
#[path = "schema2_mbx_cancel_probe_observer_fixture_tests.rs"]
mod observer_fixtures;
#[path = "schema2_mbx_cancel_probe_pre_save_fixture_tests.rs"]
mod pre_save_fixtures;
#[path = "schema2_mbx_cancel_probe_receipt_fixture_tests.rs"]
mod receipt_fixtures;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(super) fn temp_dir(label: &str) -> io::Result<PathBuf> {
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-mbx-cancel-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path)?;
    Ok(path)
}

pub(super) fn fake_bin(root: &Path) -> io::Result<PathBuf> {
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let gh = bin.join("gh");
    fs::write(&gh, FAKE_GH)?;
    let sha256sum = bin.join("sha256sum");
    fs::write(
        &sha256sum,
        "#!/usr/bin/env bash\nexec shasum -a 256 \"$@\"\n",
    )?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(&sha256sum, fs::Permissions::from_mode(0o755))?;
    Ok(bin)
}

pub(super) fn run_bash(
    script: &str,
    cwd: &Path,
    bin: &Path,
    envs: &[(String, String)],
) -> io::Result<Output> {
    let mut path = bin.as_os_str().to_os_string();
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let mut command = Command::new("bash");
    command
        .env_clear()
        .env("PATH", path)
        .env("HOME", cwd)
        .env("TMPDIR", cwd)
        .current_dir(cwd)
        .args(["-c", script]);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output()
}

const FAKE_GH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
method=GET
include=false
input=
output=
endpoint=
hostname=
while (($#)); do
  case "$1" in
    --method) method="$2"; shift 2 ;;
    --input) input="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --include) include=true; shift ;;
    --hostname) hostname="$2"; shift 2 ;;
    *) endpoint="$1"; shift ;;
  esac
done
test "$hostname" = github.com
printf '%s %s\n' "$method" "$endpoint" >> "$GH_LOG"
case "$method:$endpoint" in
  GET:/repos/tailrocks/velnor-new/actions/workflows/qualification.yml)
    printf '%s\n' '{"id":77,"path":".github/workflows/qualification.yml","state":"active"}' ;;
  POST:/repos/tailrocks/velnor-new/actions/workflows/qualification.yml/dispatches)
    jq -e --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" \
      '.ref == "refs/heads/main" and .return_run_details == true
       and .inputs.mode == $mode and .inputs.probe_id == $probe' "$input" >/dev/null
    if [ "${GH_MODE:-good}" = missing-id ]; then
      printf 'HTTP/2 200 OK\r\n\r\n%s\n' \
        '{"run_url":"https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123"}'
    elif [ "${GH_MODE:-good}" = bad-url ]; then
      printf 'HTTP/2 200 OK\r\n\r\n%s\n' \
        '{"workflow_run_id":123,"run_url":"https://api.github.com/repos/other/repo/actions/runs/123"}'
    else
      printf 'HTTP/2 200 OK\r\n\r\n%s\n' \
        '{"workflow_run_id":123,"run_url":"https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123"}'
    fi ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/900)
    jq -cn --arg sha "$GITHUB_SHA" --arg actor "$GITHUB_ACTOR" \
      --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" \
      --argjson workflow "${OBSERVER_WORKFLOW_ID:-77}" \
      '{id:900,workflow_id:$workflow,path:".github/workflows/qualification.yml@refs/heads/main",
        repository:{full_name:"tailrocks/velnor-new"},head_repository:{full_name:"tailrocks/velnor-new"},
        event:"workflow_dispatch",head_branch:"main",head_sha:$sha,run_attempt:1,
        display_title:("MBX cancellation " + $mode + " " + $probe),actor:{login:$actor}}' ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/900/attempts/1/jobs?per_page=100)
    jq -cn --arg name "$OBSERVER_JOB_NAME" \
      '{jobs:[{id:901,name:$name,status:"in_progress",steps:[
        {name:"Set up job",status:"completed",conclusion:"success",number:1},
        {name:"Prepare MBX bundle key",status:"completed",conclusion:"success"},
        {name:"Restore MBX single bundle",status:"completed",conclusion:"success",number:13}]}]}' ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/123)
    if [ "$GH_MODE" = mismatch ]; then repo=attacker/repo; else repo=tailrocks/velnor-new; fi
    run_count=0
    if [ -s "$GH_STATE.run-count" ]; then IFS= read -r run_count < "$GH_STATE.run-count"; fi
    run_count=$((run_count + 1))
    printf '%s\n' "$run_count" > "$GH_STATE.run-count"
    actor=github-actions[bot]
    run_sha="$GITHUB_SHA"
    attempt=1
    if [ "$GH_MODE" = initial-mismatch ] && [ "$run_count" = 1 ]; then
      actor=unexpected-actor
      run_sha=dddddddddddddddddddddddddddddddddddddddd
    fi
    if [ "$GH_MODE" = revalidate-actor ] && [ "$run_count" -ge 5 ]; then actor=unexpected-actor; fi
    if [ "$GH_MODE" = revalidate-attempt ] && [ "$run_count" -ge 5 ]; then attempt=2; fi
    status=in_progress
    conclusion=null
    if [[ "$GH_MODE" == observer-* ]]; then status=completed; conclusion=\"cancelled\"; fi
    jq -cn --arg repo "$repo" --arg sha "$run_sha" --arg mode "$VICTIM_MODE" \
      --arg probe "$PROBE_ID" --arg status "$status" --argjson conclusion "$conclusion" \
      --arg actor "$actor" --argjson attempt "$attempt" \
      '{id:123,workflow_id:77,path:".github/workflows/qualification.yml@refs/heads/main",
        repository:{full_name:$repo},head_repository:{full_name:"tailrocks/velnor-new"},
        event:"workflow_dispatch",head_branch:"main",head_sha:$sha,run_attempt:$attempt,
        display_title:("MBX cancellation " + $mode + " " + $probe),
        actor:{login:$actor},status:$status,conclusion:$conclusion}' ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/123/attempts/1/jobs?per_page=100)
    case "${GH_MODE:-good}" in
    observer-window)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{jobs:[{id:456,name:$name,status:"completed",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13,started_at:"2026-10-04T00:00:00Z"}]}]}'
      ;;
    observer-duplicate-save)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{jobs:[{id:456,name:$name,status:"completed",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13,started_at:"2026-10-04T00:00:00Z"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:14,started_at:"2026-10-04T00:00:00Z"}]}]}'
      ;;
    observer-missing-save)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{jobs:[{id:456,name:$name,status:"completed",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"}]}]}'
      ;;
    observer-object-steps)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{jobs:[{id:456,name:$name,status:"completed",steps:{save:{
          name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13,started_at:"2026-10-04T00:00:00Z"}}}]}'
      ;;
    *)
      if [ "$PROBE_PHASE" = pre-save ]; then
        jq -cn --arg name "$VICTIM_JOB_NAME" \
          '{jobs:[{id:456,name:$name,status:"in_progress",steps:[
            {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
            {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
            {name:"Wait at MBX pre-save cancellation point",status:"in_progress",conclusion:null},
            {name:"Save MBX single bundle",status:"completed",conclusion:"skipped"}]}]}'
        exit 0
      fi
      jobs_count=0
      if [ -s "$GH_STATE.jobs-count" ]; then IFS= read -r jobs_count < "$GH_STATE.jobs-count"; fi
      jobs_count=$((jobs_count + 1))
      printf '%s\n' "$jobs_count" > "$GH_STATE.jobs-count"
      job_status=in_progress
      save_status=in_progress
      save_conclusion=null
      if [ "$GH_MODE" = revalidate-job ] && [ "$jobs_count" -ge 3 ]; then
        job_status=completed
        save_status=completed
        save_conclusion=cancelled
      fi
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        --arg job_status "$job_status" --arg save_status "$save_status" \
        --argjson save_conclusion "$save_conclusion" \
        '{jobs:[{id:456,name:$name,status:$job_status,steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:$save_status,conclusion:$save_conclusion,number:13}]}]}'
      ;;
    esac ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/123/artifacts?per_page=100)
    artifacts_count=0
    if [ -s "$GH_STATE.artifacts-count" ]; then IFS= read -r artifacts_count < "$GH_STATE.artifacts-count"; fi
    artifacts_count=$((artifacts_count + 1))
    printf '%s\n' "$artifacts_count" > "$GH_STATE.artifacts-count"
    artifact_id=55
    if [ "$GH_MODE" = revalidate-artifact ] && [ "$artifacts_count" -ge 2 ]; then artifact_id=56; fi
    jq -cn --arg name "$VICTIM_ARTIFACT_NAME" --arg digest "$GH_ARTIFACT_DIGEST" \
      --argjson artifact_id "$artifact_id" \
      --argjson size "$GH_ARTIFACT_SIZE" \
      '{artifacts:[{id:$artifact_id,name:$name,expired:false,size_in_bytes:$size,
        digest:("sha256:" + $digest),workflow_run:{id:123}}]}' ;;
  GET:/repos/tailrocks/velnor-new/actions/artifacts/55/zip)
    test -n "$output"
    cp "$GH_ARTIFACT_ZIP" "$output" ;;
  GET:/repos/tailrocks/velnor-new/actions/caches?key=*)
    case "${GH_MODE:-good}" in
      cache-object) printf '%s\n' '{"actions_caches":{}}' ;;
      cache-null-response) printf '%s\n' 'null' ;;
      cache-null-entry) printf '%s\n' '{"actions_caches":[null]}' ;;
      cache-missing-array) printf '%s\n' '{"count":0,"caches":[]}' ;;
      cache-valid-record)
        cache_key="${endpoint#*key=}"
        cache_key="${cache_key%%&*}"
        jq -cn --arg key "$cache_key" \
          '{actions_caches:[{id:5,key:$key,ref:"refs/heads/main",size_in_bytes:1024,last_accessed_at:null}]}' ;;
      *) printf '%s\n' '{"actions_caches":[]}' ;;
    esac ;;
  POST:/repos/tailrocks/velnor-new/actions/runs/123/cancel)
    test "$include" = true
    printf 'HTTP/2 202 Accepted\r\n\r\n' ;;
  *) printf 'unexpected fixture request: %s %s\n' "$method" "$endpoint" >&2; exit 91 ;;
esac
"#;

#[test]
fn raw_api_steps_keep_multiline_source_and_scope_the_token() -> Result<(), Box<dyn Error>> {
    let env = BTreeMap::from([(
        "RUN_ID".to_owned(),
        "${{ steps.dispatch.outputs.workflow_run_id }}".to_owned(),
    )]);
    let step =
        render::token_bash_step("Dispatch", Some("dispatch"), scripts::DISPATCH, &env, None)?;
    let Yaml::Map(fields) = step else {
        return Err(io::Error::other("raw run step is not a mapping").into());
    };
    let run = fields
        .iter()
        .find(|(key, _)| key == "run")
        .map(|(_, value)| value)
        .ok_or_else(|| io::Error::other("raw run field missing"))?;
    assert_eq!(run, &Yaml::str(scripts::DISPATCH));
    let env = fields
        .iter()
        .find(|(key, _)| key == "env")
        .map(|(_, value)| value)
        .ok_or_else(|| io::Error::other("raw env field missing"))?;
    let Yaml::Map(env) = env else {
        return Err(io::Error::other("raw env is not a mapping").into());
    };
    assert!(
        env.iter().any(|(key, value)| {
            key == "GH_TOKEN" && value == &Yaml::str("${{ github.token }}")
        })
    );
    let override_token = BTreeMap::from([("GH_TOKEN".to_owned(), "bad".to_owned())]);
    assert!(render::token_bash_step("bad", None, "true", &override_token, None).is_err());
    Ok(())
}

#[test]
fn controller_artifact_download_targets_directory_and_reads_exact_file()
-> Result<(), Box<dyn Error>> {
    for (phase, artifact_name) in [
        (
            super::Phase::PreSave,
            "mbx-cancel-controller-receipt-pre-save",
        ),
        (
            super::Phase::DuringSave,
            "mbx-cancel-controller-receipt-during-save",
        ),
    ] {
        let step = super::probe_steps::download_controller_receipt_step(phase)?;
        let StepKind::Action { with, .. } = step.kind else {
            return Err(io::Error::other("receipt download is not an action").into());
        };
        assert_eq!(with.get("name").map(String::as_str), Some(artifact_name));
        assert_eq!(
            with.get("path").map(String::as_str),
            Some("${{ runner.temp }}/mbx-cancel/controller")
        );
    }
    assert!(
        scripts::VALIDATE_CONTROLLER_RECEIPT
            .contains(r#"path="$RUNNER_TEMP/mbx-cancel/controller/receipt.json""#)
    );
    Ok(())
}

#[test]
fn malformed_save_step_lists_never_request_step_logs() -> Result<(), Box<dyn Error>> {
    for mode in [
        "observer-duplicate-save",
        "observer-missing-save",
        "observer-object-steps",
    ] {
        let fixture = controller_fixtures::Fixture::new(mode, false)?;
        fixture.install_curl()?;
        let observer = fixture.root.join("mbx-cancel/observer");
        fs::create_dir_all(&observer)?;
        fs::write(
            observer.join("cache-before.json"),
            "{\"count\":0,\"caches\":[]}\n",
        )?;
        let output = fixture.root.join("evidence.output");
        fs::write(&output, "")?;
        let curl_log = fixture.root.join("curl.log");
        let summary = fixture.root.join("summary.md");
        fs::write(&summary, "")?;
        let mut env = fixture.env(&output, mode);
        env.extend([
            ("GH_TOKEN".to_owned(), "fixture-secret-token".to_owned()),
            (
                "VALIDATED_CACHE_KEY".to_owned(),
                controller_fixtures::expected_key(),
            ),
            ("CURL_LOG".to_owned(), curl_log.display().to_string()),
            ("CURL_LOCATION_MODE".to_owned(), "missing".to_owned()),
            ("CHILD_SOURCE_SHA".to_owned(), "c".repeat(40)),
            ("CHILD_ACTOR".to_owned(), "github-actions[bot]".to_owned()),
            ("CONTROLLER_CANCEL_REQUESTED".to_owned(), "true".to_owned()),
            (
                "CONTROLLER_CANCEL_AT".to_owned(),
                "2026-10-04T00:00:10Z".to_owned(),
            ),
            ("CONTROLLER_BEFORE_COUNT".to_owned(), "0".to_owned()),
            (
                "GITHUB_STEP_SUMMARY".to_owned(),
                summary.display().to_string(),
            ),
        ]);
        let script = scripts::observer_evidence();
        let result = run_bash(&script, &fixture.root, &fixture.bin, &env)?;
        assert!(
            result.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            fs::read_to_string(observer.join("child-evidence.json"))?
                .contains("\"upload_started_before_cancel\":false")
        );
        let requests = if curl_log.exists() {
            fs::read_to_string(&curl_log)?
        } else {
            String::new()
        };
        assert!(requests.is_empty(), "{mode} called curl: {requests}");
        fs::remove_dir_all(fixture.root)?;
    }
    Ok(())
}
