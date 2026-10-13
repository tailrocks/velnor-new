//! Exact-run API evidence and overlap receipt for the parallel MBX probe.

use velnor_actions_contract::Step;

use super::{NEW_KEY_SCOPE, SHARED_SCOPE};
use crate::RenderError;
use crate::schema2::MbxQualificationPins;
use crate::steps;
use crate::yaml::Yaml;

pub(super) const API_STEP_NAME: &str = "Validate MBX parallel REST timestamps";
pub(super) const LAST_RECEIPT_STEP_NAME: &str = "Download MBX new-key-writer receipt";

const API_RECEIPT_SCRIPT: &str = r#"set -euo pipefail
set -o noclobber
umask 077
case "$RUNNER_TEMP" in /*) ;; *) echo 'invalid runner temp path' >&2; exit 1 ;; esac
case "$GITHUB_RUN_ID" in ''|*[!0-9]*) echo 'invalid current run ID' >&2; exit 1 ;; esac
case "$GITHUB_RUN_ATTEMPT" in ''|*[!0-9]*) echo 'invalid current run attempt' >&2; exit 1 ;; esac
parent_identity="$RUNNER_TEMP/.mbx-parallel-parent-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$$"
cd "$RUNNER_TEMP" || exit 1
pwd -P > "$parent_identity"
IFS= read -r runner_temp_real < "$parent_identity"
rm -f "$parent_identity"
cd "$runner_temp_real" || exit 1
input="$runner_temp_real/mbx-parallel-input"
evidence="$runner_temp_real/mbx-cache-evidence"
identity="$runner_temp_real/.mbx-parallel-identity-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$$"
verify_directory() {
  local directory="$1" expected="$2" mode="$3" resolved
  if [ ! -d "$directory" ] || [ -L "$directory" ] || [ ! -O "$directory" ]; then
    echo 'invalid qualification directory' >&2
    return 1
  fi
  cd "$directory" || return 1
  pwd -P > "$identity"
  IFS= read -r resolved < "$identity"
  rm -f "$identity"
  cd "$runner_temp_real" || return 1
  if [ "$resolved" != "$expected" ]; then
    echo 'qualification directory escaped canonical runner temp' >&2
    return 1
  fi
  if [ "$mode" = 700 ]; then
    find "$directory" -prune -type d -perm 700 -print -quit | grep -Fqx "$directory"
  fi
}
verify_directory "$input" "$runner_temp_real/mbx-parallel-input" ''
verify_directory "$evidence" "$runner_temp_real/mbx-cache-evidence" 700
seed="$input/seed/cache-receipt.json"
reader_a="$input/reader-a/cache-receipt.json"
reader_b="$input/reader-b/cache-receipt.json"
new_writer="$input/new-key-writer/cache-receipt.json"
jobs="$input/current-attempt-jobs.json"
seed_cache="$input/seed-cache-api.json"
new_cache="$input/new-cache-api.json"
receipt="$evidence/parallel-api-receipt.json"

for file in "$jobs" "$seed_cache" "$new_cache" "$receipt"; do
  if [ -e "$file" ] || [ -L "$file" ]; then
    echo 'qualification receipt path already exists' >&2
    exit 1
  fi
done
test "$GITHUB_EVENT_NAME" = workflow_dispatch
test "$GITHUB_REF" = refs/heads/main
test "$GITHUB_WORKFLOW_REF" = "$GITHUB_REPOSITORY/.github/workflows/qualification.yml@refs/heads/main"
case "$GITHUB_SHA" in ''|*[!0-9a-f]*) echo 'invalid current source SHA' >&2; exit 1 ;; esac
test "${#GITHUB_SHA}" -eq 40

verify_receipt() {
  local file="$1" job="$2" role="$3" scope="$4" directory="${1%/*}"
  verify_directory "$directory" "$input/${directory##*/}" ''
  if [ ! -f "$file" ] || [ -L "$file" ] || [ ! -O "$file" ]; then
    echo 'invalid cache receipt file' >&2
    return 1
  fi
  jq -e --arg job "$job" --arg role "$role" --arg scope "$scope" --arg run "$GITHUB_RUN_ID" --arg attempt "$GITHUB_RUN_ATTEMPT" --arg sha "$GITHUB_SHA" --arg ref "$GITHUB_REF" --arg workflow "$GITHUB_WORKFLOW_REF" --arg action "$MBX_EXPECTED_ACTION_REF" --arg version "$MBX_EXPECTED_VERSION" --arg rust "$MBX_EXPECTED_RUST_VERSION" '.job_id == $job and .role == $role and .scope == $scope and .run_id == $run and .run_attempt == $attempt and .source_sha == $sha and .source_ref == $ref and .workflow_ref == $workflow and .mbx_action_ref == $action and .mbx_version == $version and .rust_version == $rust and (.primary_key | type == "string" and length > 0) and (.cache_hit | type == "string") and (.imported_objects | type == "number") and (.cached_compilations | type == "number")' "$file" >/dev/null
}

verify_receipt "$seed" mbx-parallel-seed seed "$MBX_EXPECTED_SHARED_SCOPE"
verify_receipt "$reader_a" mbx-parallel-reader-a reader-a "$MBX_EXPECTED_SHARED_SCOPE"
verify_receipt "$reader_b" mbx-parallel-reader-b reader-b "$MBX_EXPECTED_SHARED_SCOPE"
verify_receipt "$new_writer" mbx-parallel-new-key-writer new-key-writer "$MBX_EXPECTED_NEW_KEY_SCOPE"

jq -er '.primary_key' "$seed" > "$input/seed-primary-key.txt"
jq -er '.primary_key' "$reader_a" > "$input/reader-a-primary-key.txt"
jq -er '.primary_key' "$reader_b" > "$input/reader-b-primary-key.txt"
jq -er '.primary_key' "$new_writer" > "$input/new-writer-primary-key.txt"
IFS= read -r seed_key < "$input/seed-primary-key.txt"
IFS= read -r reader_a_key < "$input/reader-a-primary-key.txt"
IFS= read -r reader_b_key < "$input/reader-b-primary-key.txt"
IFS= read -r new_key < "$input/new-writer-primary-key.txt"
test "$seed_key" = "$reader_a_key"
test "$seed_key" = "$reader_b_key"
test "$seed_key" != "$new_key"
case "$seed_key:$new_key" in *"run-$GITHUB_RUN_ID-attempt-$GITHUB_RUN_ATTEMPT-"*) ;; *) echo 'parallel key is not bound to this run attempt' >&2; exit 1 ;; esac

jq -e '.cache_hit == "false" and (.matched_key == null or .matched_key == "") and .export_ready == "true" and .save_outcome == "success"' "$seed" >/dev/null
jq -e '.cache_hit == "false" and (.matched_key == null or .matched_key == "") and .export_ready == "true" and .save_outcome == "success"' "$new_writer" >/dev/null
jq -e '.cache_hit == "true" and .matched_key == .primary_key and .imported_objects > 0 and .cached_compilations > 0' "$reader_a" >/dev/null
jq -e '.cache_hit == "true" and .matched_key == .primary_key and .imported_objects > 0 and .cached_compilations > 0' "$reader_b" >/dev/null

verify_directory "$input" "$runner_temp_real/mbx-parallel-input" ''
verify_directory "$evidence" "$runner_temp_real/mbx-cache-evidence" 700
gh api -X GET "repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/attempts/$GITHUB_RUN_ATTEMPT/jobs" -f per_page=100 > "$jobs"
gh api -X GET "repos/$GITHUB_REPOSITORY/actions/caches" -f "key=$seed_key" -f "ref=$GITHUB_REF" -f per_page=100 > "$seed_cache"
gh api -X GET "repos/$GITHUB_REPOSITORY/actions/caches" -f "key=$new_key" -f "ref=$GITHUB_REF" -f per_page=100 > "$new_cache"
jq -e --arg key "$seed_key" --arg ref "$GITHUB_REF" '[.actions_caches[] | select(.key == $key and .ref == $ref)] | length == 1' "$seed_cache" >/dev/null
jq -e --arg key "$new_key" --arg ref "$GITHUB_REF" '[.actions_caches[] | select(.key == $key and .ref == $ref)] | length == 1' "$new_cache" >/dev/null

jq -e -n --slurpfile api "$jobs" --slurpfile seed_receipt "$seed" --slurpfile reader_a_receipt "$reader_a" --slurpfile reader_b_receipt "$reader_b" --slurpfile new_writer_receipt "$new_writer" --slurpfile seed_caches "$seed_cache" --slurpfile new_caches "$new_cache" --arg run "$GITHUB_RUN_ID" --arg attempt "$GITHUB_RUN_ATTEMPT" --arg sha "$GITHUB_SHA" --arg ref "$GITHUB_REF" --arg workflow "$GITHUB_WORKFLOW_REF" --arg action "$MBX_EXPECTED_ACTION_REF" --arg mbx "$MBX_EXPECTED_VERSION" --arg rust "$RUSTUP_TOOLCHAIN" '
  def one_job($name):
    [$api[0].jobs[] | select(.name == $name)] as $rows
    | if ($rows | length) == 1 then $rows[0] else error("missing_or_duplicate_job:" + $name) end;
  def one_step($job; $name):
    [$job.steps[] | select(.name == $name)] as $rows
    | if ($rows | length) == 1 then $rows[0] else error("missing_or_duplicate_step:" + $name) end;
  def successful($record; $label):
    if $record.status == "completed" and $record.conclusion == "success"
    then $record else error("not_successful:" + $label) end;
  def epoch_ms:
    if type != "string" then error("missing_timestamp") else
      capture("^(?<whole>[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})(?:\\.(?<fraction>[0-9]+))?Z$") as $time
      | (($time.whole + "Z") | fromdateiso8601) * 1000
        + (((($time.fraction // "") + "000")[:3]) | tonumber)
    end;
  if $api[0].total_count != ($api[0].jobs | length)
  then error("attempt_jobs_page_incomplete") else . end
  | one_job("MBX parallel / seed") as $seed_job
  | one_job("MBX parallel / reader-a") as $reader_a_job
  | one_job("MBX parallel / reader-b") as $reader_b_job
  | one_job("MBX parallel / new-key-writer") as $writer_job
  | successful($seed_job; "seed") as $seed_ok
  | successful($reader_a_job; "reader-a") as $reader_a_ok
  | successful($reader_b_job; "reader-b") as $reader_b_ok
  | successful($writer_job; "new-key-writer") as $writer_ok
  | successful(one_step($seed_ok; "Save MBX single bundle"); "seed-save") as $seed_save
  | successful(one_step($reader_a_ok; "Restore MBX single bundle"); "reader-a-restore") as $restore_a
  | successful(one_step($reader_a_ok; "Compile MBX cache probe"); "reader-a-build") as $build_a
  | successful(one_step($reader_b_ok; "Restore MBX single bundle"); "reader-b-restore") as $restore_b
  | successful(one_step($reader_b_ok; "Compile MBX cache probe"); "reader-b-build") as $build_b
  | successful(one_step($writer_ok; "Restore MBX single bundle"); "new-key-restore") as $restore_writer
  | successful(one_step($writer_ok; "Compile MBX cache probe"); "new-key-build") as $build_writer
  | [$restore_a.started_at, $restore_b.started_at, $restore_writer.started_at] as $starts
  | [$build_a.completed_at, $build_b.completed_at, $build_writer.completed_at] as $ends
  | ([$starts[] | epoch_ms] | max) as $latest_start
  | ([$ends[] | epoch_ms] | min) as $earliest_end
  | ($seed_ok.completed_at | epoch_ms) as $seed_job_end
  | ($seed_save.completed_at | epoch_ms) as $seed_save_end
  | if ($seed_job_end >= ([$starts[] | epoch_ms] | min)
        or $seed_save_end >= ([$starts[] | epoch_ms] | min))
    then error("seed_save_not_complete_before_parallel_starts") else . end
  | {
      schema_version: 1,
      classification: (if $latest_start < $earliest_end then "RUN" else "NOT_RUN" end),
      run_id: $run,
      run_attempt: $attempt,
      source_sha: $sha,
      source_ref: $ref,
      workflow_ref: $workflow,
      mbx_action_ref: $action,
      mbx_version: $mbx,
      rust_version: $rust,
      seed: {
        job_id: "mbx-parallel-seed", api_job_id: $seed_ok.id,
        completed_at: $seed_ok.completed_at, save_completed_at: $seed_save.completed_at,
        primary_key: $seed_receipt[0].primary_key
      },
      parallel_intervals: [
        {role: "reader-a", job_id: "mbx-parallel-reader-a", api_job_id: $reader_a_ok.id,
         restore_started_at: $restore_a.started_at, build_completed_at: $build_a.completed_at,
         primary_key: $reader_a_receipt[0].primary_key},
        {role: "reader-b", job_id: "mbx-parallel-reader-b", api_job_id: $reader_b_ok.id,
         restore_started_at: $restore_b.started_at, build_completed_at: $build_b.completed_at,
         primary_key: $reader_b_receipt[0].primary_key},
        {role: "new-key-writer", job_id: "mbx-parallel-new-key-writer", api_job_id: $writer_ok.id,
         restore_started_at: $restore_writer.started_at, build_completed_at: $build_writer.completed_at,
         primary_key: $new_writer_receipt[0].primary_key}
      ],
      overlap_started_at: ([$starts[] | epoch_ms] | max),
      overlap_completed_at: ([$ends[] | epoch_ms] | min),
      overlap_duration_ms: (if $latest_start < $earliest_end then $earliest_end - $latest_start else 0 end),
      exact_cache_entries: [
        ($seed_caches[0].actions_caches[] | select(.key == $seed_receipt[0].primary_key and .ref == $ref)
         | {id, key, ref, version, size_in_bytes, created_at}),
        ($new_caches[0].actions_caches[] | select(.key == $new_writer_receipt[0].primary_key and .ref == $ref)
         | {id, key, ref, version, size_in_bytes, created_at})
      ]
    }
  ' > "$receipt"
verify_directory "$evidence" "$runner_temp_real/mbx-cache-evidence" 700
jq -er '"MBX parallel overlap classification: " + .classification' "$receipt"
"#;

/// Add four pinned artifact downloads before the qualification lifecycle.
///
/// # Errors
/// Returns errors when a pinned artifact step is invalid.
pub(super) fn receipt_steps() -> Result<Vec<Step>, RenderError> {
    let input_root = "${{ runner.temp }}/mbx-parallel-input";
    let mut steps = Vec::new();
    for (role, job_id) in [
        ("seed", "mbx-parallel-seed"),
        ("reader-a", "mbx-parallel-reader-a"),
        ("reader-b", "mbx-parallel-reader-b"),
        ("new-key-writer", "mbx-parallel-new-key-writer"),
    ] {
        let path = format!("{input_root}/{role}");
        let mut download =
            steps::download_artifact_step(&format!("mbx-cache-evidence-{job_id}"), &path)?;
        download.name = format!("Download MBX {role} receipt");
        steps.push(download);
    }
    Ok(steps)
}

/// Fixed raw-YAML observer step following the native schema2 token pattern.
/// This is the only step with the narrowly scoped GitHub token binding.
pub(super) fn observer_api_step(request: &MbxQualificationPins) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(API_STEP_NAME)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
                (
                    "MBX_EXPECTED_ACTION_REF".to_owned(),
                    Yaml::str(request.mbx_action_uses.clone()),
                ),
                (
                    "MBX_EXPECTED_NEW_KEY_SCOPE".to_owned(),
                    Yaml::str(NEW_KEY_SCOPE),
                ),
                (
                    "MBX_EXPECTED_RUST_VERSION".to_owned(),
                    Yaml::str(request.rust_version.clone()),
                ),
                (
                    "MBX_EXPECTED_SHARED_SCOPE".to_owned(),
                    Yaml::str(SHARED_SCOPE),
                ),
                (
                    "MBX_EXPECTED_VERSION".to_owned(),
                    Yaml::str(request.mbx_version.clone()),
                ),
            ]),
        ),
        (
            "run".to_owned(),
            Yaml::str(API_RECEIPT_SCRIPT),
        ),
    ])
}
