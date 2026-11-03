// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_actionlint::actions::ATTEST_USES;
use velnor_actions_contract::workflow::DEPENDENCY_SEED_JOB_CONDITION;
use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily, RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_CANARY_ARTIFACT_OUTPUT, STATE_CANARY_PRODUCER_JOB_ID, STATE_CANARY_SEAL_JOB_ID,
    STATE_CANARY_UNVERIFIED_TEST, STATE_SEAL_ARTIFACT_RETENTION_DAYS, STATE_SEAL_KEY_EXPR,
    STATE_SEAL_PREDICATE, STATE_SEAL_PREDICATE_TYPE, STATE_SEAL_PREFIX_ENV,
    STATE_SEAL_SAVE_CONDITION, STATE_SEAL_SUBJECT_EXPR, STATE_SEAL_SUBJECT_REL,
    state_canary_identity_prefix,
};
use velnor_actions_contract::{
    CacheAccess, CacheWriter, Job, JobEnv, JobTimeout, PermissionLevel, Permissions, Step, StepId,
    StepRole,
};

use crate::RenderError;
use crate::render::PLAN_JOB_ID;
use crate::steps::{
    DOWNLOAD_ARTIFACT_USES, TOOLS_SAVE_USES, UPLOAD_ARTIFACT_USES, action_step, shell_step,
};

const BUILD_DISPLAY_NAME: &str = "State canary build";
const SEAL_DISPLAY_NAME: &str = "State canary seal";
const JOB_MINUTES: u32 = 10;
const CANARY_FILE_EXPR: &str = "${{ runner.temp }}/velnor/state-seal/out/payload.tar.zst";
const CANARY_FILE: &str = "$RUNNER_TEMP/velnor/state-seal/out/payload.tar.zst";
const CANARY_BYTES: usize = 1024;
const SEALED_OUTPUT: &str = "sealed";
const GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

pub fn add_state_seal_canary(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let Some(plan) = jobs.get(PLAN_JOB_ID) else {
        return Ok(());
    };
    if !plan
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::StateSelect))
    {
        return Ok(());
    }
    let build = canary_build_job(&plan.runs_on)?;
    let seal = canary_seal_job(&plan.runs_on)?;
    for (id, job) in [
        (STATE_CANARY_PRODUCER_JOB_ID, build),
        (STATE_CANARY_SEAL_JOB_ID, seal),
    ] {
        velnor_actions_contract::workflow::validate_job_id(id).map_err(RenderError::Contract)?;
        if jobs.insert(id.to_owned(), job).is_some() {
            return Err(invalid("id_collision"));
        }
    }
    Ok(())
}

fn canary_build_job(runs_on: &str) -> Result<Job, RenderError> {
    let mut build = shell_step(
        "Write canary payload",
        vec!["sh".to_owned(), "-c".to_owned(), canary_script()],
        BTreeMap::new(),
    )?;
    build.role = Some(StepRole::StateSealBuild);
    let mut upload = action_step(
        "Upload canary payload",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                "velnor-state-seal-canary-r${{ github.run_id }}-a${{ github.run_attempt }}"
                    .to_owned(),
            ),
            ("path".to_owned(), CANARY_FILE_EXPR.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                STATE_SEAL_ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    upload.id = Some(StepId::StateSealUpload);
    upload.role = Some(StepRole::StateSealUpload);
    Ok(Job {
        display_name: BUILD_DISPLAY_NAME.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::try_from(JOB_MINUTES).map_err(RenderError::Contract)?,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: Some(format!(
            "{DEPENDENCY_SEED_JOB_CONDITION} && {STATE_CANARY_UNVERIFIED_TEST}"
        )),
        permissions: None,
        cache_access: CacheAccess::Read,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::from([(
            STATE_CANARY_ARTIFACT_OUTPUT.to_owned(),
            format!(
                "${{{{ steps.{}.outputs.artifact-id }}}}",
                StepId::StateSealUpload.as_str()
            ),
        )]),
        env: JobEnv::Unset,
        steps: vec![build, upload],
    })
}

fn canary_seal_job(runs_on: &str) -> Result<Job, RenderError> {
    let mut download = action_step(
        "Download sealed payload",
        DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "artifact-ids".to_owned(),
                format!(
                    "${{{{ needs.{STATE_CANARY_PRODUCER_JOB_ID}.outputs.{STATE_CANARY_ARTIFACT_OUTPUT} }}}}"
                ),
            ),
            ("merge-multiple".to_owned(), "true".to_owned()),
            ("path".to_owned(), transport_dir()?),
        ]),
    )?;
    download.role = Some(StepRole::StateSealDownload);
    let subject = state_seal_subject_step(&state_canary_identity_prefix())?;
    let mut attest = action_step(
        "Attest state seal",
        ATTEST_USES,
        BTreeMap::from([
            (
                "subject-path".to_owned(),
                STATE_SEAL_SUBJECT_EXPR.to_owned(),
            ),
            (
                "predicate-type".to_owned(),
                STATE_SEAL_PREDICATE_TYPE.to_owned(),
            ),
            ("predicate".to_owned(), STATE_SEAL_PREDICATE.to_owned()),
        ]),
    )?;
    attest.role = Some(StepRole::StateSealAttest);
    attest.condition = Some(STATE_SEAL_SAVE_CONDITION.to_owned());
    let mut save = action_step(
        "Save sealed payload",
        TOOLS_SAVE_USES,
        BTreeMap::from([
            ("key".to_owned(), STATE_SEAL_KEY_EXPR.to_owned()),
            ("path".to_owned(), COMPLETE_STATE_TRANSPORT_EXPR.to_owned()),
        ]),
    )?;
    save.role = Some(StepRole::StateSealSave);
    save.condition = Some(STATE_SEAL_SAVE_CONDITION.to_owned());
    Ok(Job {
        display_name: SEAL_DISPLAY_NAME.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::try_from(JOB_MINUTES).map_err(RenderError::Contract)?,
        needs: vec![
            PLAN_JOB_ID.to_owned(),
            STATE_CANARY_PRODUCER_JOB_ID.to_owned(),
        ],
        condition: Some(CacheWriter::StateSeal.condition()),
        permissions: Some(Permissions {
            contents: PermissionLevel::None,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::Write,
            actions: PermissionLevel::None,
            packages: PermissionLevel::None,
            attest: PermissionLevel::Write,
        }),
        cache_access: CacheAccess::Writer(CacheWriter::StateSeal),
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: JobEnv::Unset,
        steps: vec![download, subject, attest, save],
    })
}

fn transport_dir() -> Result<String, RenderError> {
    COMPLETE_STATE_TRANSPORT_EXPR
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_owned())
        .ok_or_else(|| invalid("transport_path"))
}

fn transport_file() -> Result<String, RenderError> {
    COMPLETE_STATE_TRANSPORT_EXPR
        .strip_prefix(RUNNER_TEMP_EXPR)
        .map(|rest| format!("{RUNNER_TEMP_SHELL}{rest}"))
        .ok_or_else(|| invalid("transport_not_runner_temp"))
}

fn canary_script() -> String {
    [
        "set -eu".to_owned(),
        GUARD.to_owned(),
        format!("file=\"{CANARY_FILE}\""),
        "mkdir -p \"${file%/*}\"".to_owned(),
        format!(
            "{{ printf 'velnor state seal canary run %s attempt %s\\n' \"${{GITHUB_RUN_ID-}}\" \"${{GITHUB_RUN_ATTEMPT-}}\"; head -c {CANARY_BYTES} /dev/zero | tr '\\0' x; }} > \"$file\""
        ),
    ]
    .join("; ")
}

pub fn state_seal_subject_step(prefix: &str) -> Result<Step, RenderError> {
    let stored = CacheFamily::MbxBundle.spec().ceiling.stored();
    let script = format!(
        r#"set -eu
{GUARD}
payload="{payload}"
d="$RUNNER_TEMP/{rel_dir}"
subject="$RUNNER_TEMP/{rel}"
rm -rf "$d"
mkdir -p "$d"
refuse() {{ printf '%s\n' "::warning::cache_unavailable: state seal refused ($1)"; printf '%s\n' '{sealed}=false' >> "$GITHUB_OUTPUT"; exit 0; }}
s256() {{ sha256sum "$1" > "$d/sum" || return 1; h=; rest=; read -r h rest < "$d/sum" || [ -n "$h" ]; case "$h" in ''|*[!0-9a-f]*) return 1 ;; esac; [ "${{#h}}" -eq 64 ] || return 1; }}
prefix="${{{prefix_env}-}}"
[ -n "$prefix" ] || refuse prefix_missing
if [ -L "$payload" ] || [ ! -f "$payload" ]; then refuse payload_not_regular; fi
size=; wc -c < "$payload" > "$d/size" || refuse size_unreadable
read -r size < "$d/size" || :
case "$size" in ''|*[!0-9]*) refuse size_unreadable ;; esac
if [ "$size" -eq 0 ]; then refuse payload_empty; fi
if [ "$size" -gt {stored} ]; then refuse "over_stored stored=$size ceiling={stored}"; fi
s256 "$payload" || refuse digest_unreadable
key="${{prefix}}s256-$h"
[ "${{#key}}" -le {max_key} ] || refuse key_too_long
printf '%s\n' "$key" > "$subject"
printf '%s\n' '{sealed}=true' "key=$key" >> "$GITHUB_OUTPUT"
printf 'velnor: state seal subject %s\n' "$key"
"#,
        payload = transport_file()?,
        rel_dir = STATE_SEAL_SUBJECT_REL
            .rsplit_once('/')
            .map_or("", |(dir, _)| dir),
        rel = STATE_SEAL_SUBJECT_REL,
        sealed = SEALED_OUTPUT,
        prefix_env = STATE_SEAL_PREFIX_ENV,
        max_key = velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES,
    );
    let mut step = shell_step(
        "Write seal subject",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        BTreeMap::from([(STATE_SEAL_PREFIX_ENV.to_owned(), prefix.to_owned())]),
    )?;
    step.id = Some(StepId::StateSealSubject);
    step.role = Some(StepRole::StateSealSubject);
    Ok(step)
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("state_seal:{problem}"))
}
