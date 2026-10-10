//! Shared local composites for typed declared-task report envelopes.
//!
//! `TaskExecution` carries argv, task/report identity, and env as distinct
//! validated fields. This renderer never recovers task metadata from shell
//! source or accepts an environment marker as proof of eligibility.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, MAX_TASK_EXECUTION_ARGV, MAX_TASK_EXECUTION_ENV, MAX_TASK_EXECUTION_FRAME_BYTES, RunsOn,
    ScaleSetSelector, Step, StepKind, StepRole, TASK_EXECUTION_FRAME_MAGIC,
    TASK_EXECUTION_MANIFEST_PATH, TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1,
    TaskExecutionManifestV1, VerificationRunner,
};

use crate::{
    RenderError, action_ref::DECLARED_TASK_ACTION_PREFIX, composite, marker, steps, toolchain_env,
    tree::RenderedFile, yaml::Yaml,
};

const ACTION_NAME_PREFIX: &str = "declared-task-";
const EXECUTION_DIGEST_INPUT: &str = "digest";
const RUNTIME_RUNNER_TEMP_ENV: &str = "VELNOR_RUNTIME_RUNNER_TEMP";
const GENERATOR_VERSION_ENV: &str = "VELNOR_GENERATOR_VERSION";
const TASK_EXECUTION_DIGEST_ENV: &str = "VELNOR_TASK_EXECUTION_DIGEST";
const RUNNER_TEMP_EXPRESSION: &str = "${{ runner.temp }}";

#[path = "task_wrapper_declared.rs"]
mod declared;
#[cfg(test)]
use self::declared::declared_task_document;
use self::declared::{declared_task_call, declared_task_file, validate_task_fields};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    helper_version: String,
}

/// Replace typed task steps and emit one composite per structural shape.
///
/// Every typed step must use a supported Linux runner and follow the
/// configured checkout. A task which violates either precondition fails
/// closed instead of silently taking a shell fallback. The caller name and
/// condition remain unchanged.
pub(crate) fn factor_obligation_steps(
    jobs: &BTreeMap<String, Job>,
    checkout_uses: &str,
    generator_version: &str,
    report_helper_version: &str,
    workflow_tasks: &[crate::verification_jobs::WorkflowTaskPolicy],
    scale_set_selector: Option<&ScaleSetSelector>,
) -> Result<(BTreeMap<String, Job>, Vec<RenderedFile>), RenderError> {
    let mut eligible = BTreeMap::<(String, usize), TaskExecutionRef<'_>>::new();
    let mut shapes = BTreeSet::new();
    let mut manifest_tasks = BTreeMap::new();
    for (job_id, job) in jobs {
        for (step_index, step) in job.steps.iter().enumerate() {
            let StepKind::TaskExecution {
                argv,
                env,
                task_id,
                task_digest,
                matrix_id,
                matrix_key,
                report_helper_version: task_helper_version,
                matrix_max_parallel,
                toolchain_inputs,
            } = &step.kind
            else {
                continue;
            };
            if !supports_task_runner(job_id, &job.runs_on, workflow_tasks, scale_set_selector) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_requires_supported_linux_runner:{job_id}"
                )));
            }
            if job.check_runner.is_some()
                || !job
                    .needs
                    .iter()
                    .any(|need| need == crate::render::PLAN_JOB_ID)
            {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_job_scope_mismatch:{job_id}"
                )));
            }
            if step.id.is_some() || step.role.is_some() || step.condition.is_none() {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_authority_mismatch:{job_id}"
                )));
            }
            if task_helper_version != report_helper_version {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_helper_version_mismatch:{job_id}"
                )));
            }
            let checkout_at = job.steps[..step_index].iter().position(|previous| {
                velnor_actions_contract::workflow::step_identity::is_configured_checkout(
                    previous,
                    checkout_uses,
                )
            });
            let Some(checkout_at) = checkout_at else {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_requires_checkout:{job_id}"
                )));
            };
            let staged_binary = format!("{}{report_helper_version}", steps::STAGED_BINARY_PREFIX);
            let helper_staged_after_checkout = job.steps[checkout_at + 1..step_index]
                .iter()
                .any(|previous| helper_staged_by(previous, &staged_binary));
            if !helper_staged_after_checkout {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_requires_staged_helper:{job_id}"
                )));
            }
            validate_task_fields(argv, env, task_id, task_digest, matrix_id, matrix_key)?;
            let mut record = TaskExecutionManifestEntryV1 {
                task_id: task_id.clone(),
                execution_digest: String::new(),
                task_digest: task_digest.clone(),
                toolchain_inputs: toolchain_inputs.clone(),
                argv: argv.clone(),
                env: env.clone(),
                matrix_id: matrix_id.clone(),
                matrix_key: matrix_key.clone(),
                report_helper_version: task_helper_version.clone(),
                matrix_max_parallel: *matrix_max_parallel,
            };
            record
                .refresh_execution_digest()
                .map_err(RenderError::Contract)?;
            let execution_digest = record.execution_digest.clone();
            if manifest_tasks.insert(task_id.clone(), record).is_some() {
                return Err(RenderError::InvalidWorkflow(format!(
                    "declared_task_id_not_unique:{task_id}"
                )));
            }
            let shape = Shape {
                helper_version: task_helper_version.clone(),
            };
            shapes.insert(shape);
            eligible.insert(
                (job_id.clone(), step_index),
                TaskExecutionRef {
                    execution_digest,
                    helper_version: task_helper_version,
                },
            );
        }
    }
    let shape_ids: BTreeMap<Shape, usize> = shapes
        .into_iter()
        .enumerate()
        .map(|(index, shape)| (shape, index))
        .collect();
    let mut next = jobs.clone();
    for ((job_id, step_index), task) in &eligible {
        let shape = Shape {
            helper_version: task.helper_version.clone(),
        };
        let action_id = shape_ids.get(&shape).ok_or_else(|| {
            RenderError::InvalidWorkflow("declared_task_shape_missing".to_owned())
        })?;
        let original = jobs
            .get(job_id)
            .and_then(|job| job.steps.get(*step_index))
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_step_missing".to_owned()))?;
        let action = declared_task_call(*action_id, task, original)?;
        let job = next
            .get_mut(job_id)
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_job_missing".to_owned()))?;
        let caller = job
            .steps
            .get_mut(*step_index)
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_step_missing".to_owned()))?;
        *caller = action;
    }
    let mut files = Vec::with_capacity(shape_ids.len() + usize::from(!manifest_tasks.is_empty()));
    for (shape, action_id) in &shape_ids {
        files.push(declared_task_file(*action_id, shape, generator_version)?);
    }
    if !manifest_tasks.is_empty() {
        let manifest = TaskExecutionManifestV1 {
            schema: TASK_EXECUTION_MANIFEST_SCHEMA,
            generator_version: generator_version.to_owned(),
            tasks: manifest_tasks,
        };
        let bytes = manifest.marked_json().map_err(RenderError::Contract)?;
        steps::scan_for_private_subcommands(&bytes)?;
        files.push(RenderedFile {
            path: TASK_EXECUTION_MANIFEST_PATH.to_owned(),
            bytes,
        });
    }
    Ok((next, files))
}

/// Admit a hosted Ubuntu catalog runner or the exact Scale Set resolved from
/// the validated Linux/amd64 execution profile. Crate-obligation jobs share
/// that profile without being workflow-task jobs themselves. A selector's
/// label alone never proves the platform.
fn supports_task_runner(
    job_id: &str,
    runs_on: &str,
    workflow_tasks: &[crate::verification_jobs::WorkflowTaskPolicy],
    scale_set_selector: Option<&ScaleSetSelector>,
) -> bool {
    match RunsOn::parse(runs_on) {
        Ok(RunsOn::Hosted(label)) => {
            label.starts_with("ubuntu-")
                && velnor_actions_contract::config::RUNNER_LABEL_CATALOG.contains(&label.as_str())
        }
        Ok(RunsOn::ScaleSet(selector)) => {
            let Some(configured) = scale_set_selector else {
                return false;
            };
            if configured != &selector {
                return false;
            }
            workflow_tasks
                .iter()
                .find(|task| task.owns_job_id(job_id))
                .is_none_or(|task| {
                    let crate::verification_jobs::WorkflowTaskPolicy::Verification(policy) = task
                    else {
                        return false;
                    };
                    policy.task.runner == VerificationRunner::LinuxX64
                        && policy.runner_label == VerificationRunner::LinuxX64.runs_on()
                        && policy.scale_set_token.as_deref() == Some(runs_on)
                })
        }
        Err(_) => false,
    }
}

fn helper_staged_by(step: &Step, staged_binary: &str) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    (crate::closure::is_acquire_step(step) || step.role == Some(StepRole::PreseedStage))
        && run.iter().any(|argument| argument.contains(staged_binary))
}

struct TaskExecutionRef<'a> {
    execution_digest: String,
    helper_version: &'a String,
}

fn task_script(helper_version: &str) -> String {
    let mut script = String::new();
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
    script.push_str("\"");
    script.push_str(helper_version);
    script.push_str("\" ]] || fail_frame\n");
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

argv=()
for ((index = 0; index < argv_count; index++)); do
  value=${frame[$((10 + index))]}
  [[ "$value" != *'${{'* ]] || fail_frame
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
  [[ "$value" != *'${{'* ]] || fail_frame
  task_env+=("$key=$value")
done
[[ -n "$VELNOR_RUNTIME_RUNNER_TEMP" ]] || fail_frame
unset VELNOR_INTERNAL_OP VELNOR_GENERATOR_VERSION VELNOR_TASK_EXECUTION_DIGEST VELNOR_RUNTIME_RUNNER_TEMP
export VELNOR_TASK_ID="${frame[1]}"
export VELNOR_TASK_DIGEST="${frame[3]}"
export VELNOR_MATRIX_ID="${frame[4]}"
export VELNOR_MATRIX_KEY="${frame[5]}"
set +e
started_ms=$(date +%s%3N)
env -- "${task_env[@]}" "${argv[@]}"
task_code=$?
VELNOR_EXIT_CODE="$task_code" VELNOR_START_MS="$started_ms" VELNOR_INTERNAL_OP=write-task-report-v1 "$helper"
report_code=$?
if [ "$task_code" -ne 0 ]; then exit "$task_code"; fi
exit "$report_code"
"#,
    );
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

#[cfg(test)]
#[path = "task_wrapper_tests.rs"]
mod tests;
