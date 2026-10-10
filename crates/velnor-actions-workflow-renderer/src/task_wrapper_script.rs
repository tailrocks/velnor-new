use crate::toolchain_env;
use velnor_actions_contract::{
    MAX_TASK_EXECUTION_ARGV, MAX_TASK_EXECUTION_ENV, MAX_TASK_EXECUTION_FRAME_BYTES,
    TASK_EXECUTION_FRAME_MAGIC,
};

pub(super) fn task_script(helper_version: &str) -> String {
    let mut script = String::new();
    append_setup(&mut script, helper_version);
    append_frame_header(&mut script, helper_version);
    append_frame_counts(&mut script);
    append_argv_checks(&mut script);
    append_env_checks(&mut script);
    append_task_run(&mut script);
    replace_frame_limits(script)
}

fn append_setup(script: &mut String, helper_version: &str) {
    script.push_str("unset ");
    script.push_str(&toolchain_env::CREDENTIAL_UNSET_VARS.join(" "));
    script.push_str(" VELNOR_TASK_ID");
    script.push_str(
        r#";
set -euo pipefail
fail_frame() { printf '%s\n' 'invalid declared-task execution frame' >&2; exit 125; }
frame_file=$(mktemp "${TMPDIR:-/tmp}/velnor-task-frame.XXXXXXXX")
trap 'rm -f "$frame_file"' EXIT
helper="$RUNNER_TEMP/velnor/bin/velnor-actions-"#,
    );
    script.push_str(helper_version);
    script.push_str("\"\n");
}

fn append_frame_header(script: &mut String, helper_version: &str) {
    script.push_str(
        r#"
if ! env VELNOR_INTERNAL_OP=resolve-task-execution-v1 "$helper" > "$frame_file"; then fail_frame; fi
frame_bytes=$(wc -c < "$frame_file")
frame_bytes=${frame_bytes//[[:space:]]/}
[[ "$frame_bytes" =~ ^[0-9]{1,7}$ ]] && (( frame_bytes <= @MAX_FRAME_BYTES@ )) || fail_frame
last_byte=$(tail -c 1 "$frame_file" | od -An -t x1)
last_byte=${last_byte//[[:space:]]/}
[[ "$last_byte" == 00 ]] || fail_frame
frame=()
while IFS= read -r -d '' field; do frame+=("$field"); done < "$frame_file"
(( ${#frame[@]} >= 13 )) || fail_frame
[[ "${frame[0]}" == @FRAME_MAGIC@ ]] || fail_frame
[[ "${frame[2]}" == "$VELNOR_TASK_EXECUTION_DIGEST" ]] || fail_frame
[[ "${frame[1]}" =~ ^(stack|internal)/[a-z0-9._/-]+$ ]] || fail_frame
[[ "${frame[2]}" =~ ^b3-[0-9a-f]{64}$ ]] || fail_frame
[[ "${frame[3]}" =~ ^b3-[0-9a-f]{64}$ ]] || fail_frame
[[ "${frame[4]}" =~ ^stack:[a-z0-9._-]+\|task:(stack|internal)/[a-z0-9._/-]+$ ]] || fail_frame
[[ "${frame[5]}" =~ ^m-[0-9a-f]{16}$ ]] || fail_frame
[[ "${frame[4]#*|task:}" == "${frame[1]}" ]] || fail_frame
[[ "${frame[6]}" == "#,
    );
    script.push('"');
    script.push_str(helper_version);
    script.push_str("\" ]] || fail_frame\n");
}

fn append_frame_counts(script: &mut String) {
    script.push_str(
        r#"
case "${frame[7]}" in
  0) [[ -z "${frame[8]}" ]] || fail_frame ;;
  1) [[ "${frame[8]}" =~ ^[1-9][0-9]{0,9}$ ]] && (( frame[8] <= @U32_MAX@ )) || fail_frame ;;
  *) fail_frame ;;
esac
[[ "${frame[9]}" =~ ^[1-9][0-9]{0,2}$ ]] || fail_frame
argv_count=${frame[9]}
(( argv_count <= @MAX_ARGV@ )) || fail_frame
env_count_position=$((10 + argv_count))
(( ${#frame[@]} > env_count_position )) || fail_frame
[[ "${frame[env_count_position]}" =~ ^(0|[1-9][0-9]?)$ ]] || fail_frame
env_count=${frame[env_count_position]}
(( env_count <= @MAX_ENV@ )) || fail_frame
expected_fields=$((env_count_position + 2 + env_count * 2))
(( ${#frame[@]} == expected_fields )) || fail_frame
end_position=$((expected_fields - 1))
[[ "${frame[end_position]}" == END ]] || fail_frame
"#,
    );
}

fn append_argv_checks(script: &mut String) {
    script.push_str(
        r#"
argv=()
for ((index = 0; index < argv_count; index++)); do
  value=${frame[$((10 + index))]}
  [[ "$value" != *'$''{{'* ]] || fail_frame
  argv+=("$value")
done

task_env=(
  "VELNOR_TASK_ID=${frame[1]}"
  "VELNOR_TASK_DIGEST=${frame[3]}"
  "VELNOR_MATRIX_ID=${frame[4]}"
  "VELNOR_MATRIX_KEY=${frame[5]}"
)
seen_keys=()
for ((index = 0; index < env_count; index++)); do
  key_position=$((env_count_position + 1 + index * 2))
  key=${frame[key_position]}
  value=${frame[$((key_position + 1))]}
  [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || fail_frame
  if (( ${#seen_keys[@]} > 0 )); then
    for seen_key in "${seen_keys[@]}"; do [[ "$seen_key" != "$key" ]] || fail_frame; done
  fi
  seen_keys+=("$key")
  case "$key" in
    VELNOR_TASK_ID|VELNOR_TASK_DIGEST|VELNOR_MATRIX_ID|VELNOR_MATRIX_KEY|VELNOR_INTERNAL_OP|VELNOR_GENERATOR_VERSION|VELNOR_TASK_EXECUTION_DIGEST|VELNOR_RUNTIME_RUNNER_TEMP|"#,
    );
    script.push_str(&toolchain_env::STEP_CREDENTIAL_DENYLIST.join("|"));
    script.push_str(
        r#"|CARGO_REGISTRIES_*|*_TOKEN|ACTIONS_ID_TOKEN_REQUEST_URL|GH_HOST|GH_CONFIG_DIR|CHECKPOINT_*) fail_frame ;;
    TF_*)
      case "$key" in TF_IN_AUTOMATION|TF_INPUT) ;; *) fail_frame ;; esac ;;
  esac
  [[ "$value" != *'$''{{'* ]] || fail_frame
  task_env+=("$key=$value")
done
"#,
    );
}

fn append_env_checks(script: &mut String) {
    script.push_str(
        r#"[[ -n "$VELNOR_RUNTIME_RUNNER_TEMP" ]] || fail_frame
unset VELNOR_INTERNAL_OP VELNOR_GENERATOR_VERSION VELNOR_TASK_EXECUTION_DIGEST VELNOR_RUNTIME_RUNNER_TEMP
export VELNOR_TASK_ID="${frame[1]}"
export VELNOR_TASK_DIGEST="${frame[3]}"
export VELNOR_MATRIX_ID="${frame[4]}"
export VELNOR_MATRIX_KEY="${frame[5]}"
"#,
    );
}

fn append_task_run(script: &mut String) {
    script.push_str(
        r#"set +e
started_ms=$(date +%s%3N)
env -- "${task_env[@]}" "${argv[@]}"
task_code=$?
VELNOR_EXIT_CODE="$task_code" VELNOR_START_MS="$started_ms" VELNOR_INTERNAL_OP=write-task-report-v1 "$helper"
report_code=$?
if [ "$task_code" -ne 0 ]; then exit "$task_code"; fi
exit "$report_code"
"#,
    );
}

fn replace_frame_limits(script: String) -> String {
    script
        .replace("@FRAME_MAGIC@", TASK_EXECUTION_FRAME_MAGIC)
        .replace(
            "@MAX_FRAME_BYTES@",
            &MAX_TASK_EXECUTION_FRAME_BYTES.to_string(),
        )
        .replace("@MAX_ARGV@", &MAX_TASK_EXECUTION_ARGV.to_string())
        .replace("@MAX_ENV@", &MAX_TASK_EXECUTION_ENV.to_string())
        .replace("@U32_MAX@", &u32::MAX.to_string())
}
