use std::collections::BTreeMap;

use velnor_actions_actionlint::actions::ATTEST_USES;
use velnor_actions_contract::workflow::cache_family::state_snapshot_plan_expr;
use velnor_actions_contract::workflow::cache_family::{
    COMPLETE_STATE_TRANSPORT_EXPR, CacheFamily, RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL, RootSlug,
    StateScope, StateTrust, StateUnit, outer_entry_bound,
};
use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_ROOT_ENV;
use velnor_actions_contract::workflow::state_seal::{
    STATE_PRODUCER_ARTIFACT_OUTPUT, STATE_SEAL_ATTEST_CONDITION, STATE_SEAL_BIND_ATTEMPTS,
    STATE_SEAL_BIND_CONDITION, STATE_SEAL_BIND_PAUSE_SECONDS, STATE_SEAL_KEY_ENV,
    STATE_SEAL_KEY_EXPR, STATE_SEAL_LIST_FIELDS, STATE_SEAL_LIST_JQ, STATE_SEAL_LIST_LIMIT,
    STATE_SEAL_LIST_TAB, STATE_SEAL_PREDICATE, STATE_SEAL_PREDICATE_TYPE, STATE_SEAL_PREFIX_ENV,
    STATE_SEAL_SAVE_CONDITION, STATE_SEAL_SUBJECT_EXPR, STATE_SEAL_SUBJECT_REL,
    state_compat_plan_expr, state_produce_plan_expr, state_producer_job_id, state_seal_condition,
    state_seal_job_id,
};
use velnor_actions_contract::workflow::{ProducerKind, SOURCES_PRODUCER_JOB_ID};
use velnor_actions_contract::{
    CacheAccess, CacheWriter, Job, JobEnv, JobTimeout, PermissionLevel, Permissions, Step, StepId,
    StepKind, StepRole,
};

use crate::RenderError;
use crate::render::PLAN_JOB_ID;
use crate::steps::{
    DOWNLOAD_ARTIFACT_USES, TOOLS_SAVE_USES, action_step, ambient_shell_step, shell_step,
};

const SEAL_DISPLAY_PREFIX: &str = "State seal / ";
const JOB_MINUTES: u32 = 15;
const SEALED_OUTPUT: &str = "sealed";
const BOUND_OUTPUT: &str = "bound";
const TOKEN_EXPR: &str = "${{ github.token }}";
const GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

#[path = "state_producer_steps.rs"]
pub mod producer_steps;
#[path = "state_seal_save.rs"]
pub mod save;
#[path = "state_skeleton.rs"]
pub mod skeleton;

struct ProducerRoot {
    slug: RootSlug,
    producer_id: String,
    runs_on: String,
}

pub fn add_state_seal_jobs(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let roots = producer_roots(jobs)?;
    if roots.is_empty() {
        return Ok(());
    }
    let writers: Vec<String> = jobs
        .iter()
        .filter(|(id, job)| {
            matches!(
                job.producer_kind(),
                Some(
                    ProducerKind::SourcesProducer
                        | ProducerKind::ToolsIdentity
                        | ProducerKind::TofuProviders
                )
            ) && !id.is_empty()
        })
        .map(|(id, _)| id.clone())
        .collect();
    chain_tools_after_sources(jobs);
    let mut previous: Option<String> = None;
    let unit_bound = outer_entry_bound(CacheFamily::MbxBundle.spec().ceiling.stored())
        .ok_or_else(|| invalid("outer_bound_unbounded"))?;
    for (index, root) in roots.iter().enumerate() {
        let pending: Vec<String> = roots[index + 1..]
            .iter()
            .map(|later| {
                format!(
                    "{}:{}",
                    state_produce_plan_expr("union", later.slug.as_str()),
                    scope_prefix(&later.slug)
                )
            })
            .collect();
        let seal_id = state_seal_job_id(&root.slug);
        let mut needs = vec![PLAN_JOB_ID.to_owned(), root.producer_id.clone()];
        match &previous {
            Some(earlier) => needs.push(earlier.clone()),
            None => needs.extend(writers.iter().cloned()),
        }
        let job = seal_job(root, needs, &pending, unit_bound)?;
        velnor_actions_contract::workflow::validate_job_id(&seal_id)
            .map_err(RenderError::Contract)?;
        if jobs.insert(seal_id.clone(), job).is_some() {
            return Err(invalid("id_collision"));
        }
        previous = Some(seal_id);
    }
    Ok(())
}

fn chain_tools_after_sources(jobs: &mut BTreeMap<String, Job>) {
    if !jobs.contains_key(SOURCES_PRODUCER_JOB_ID) {
        return;
    }
    for job in jobs.values_mut() {
        if job.producer_kind() == Some(ProducerKind::ToolsIdentity) {
            job.needs = vec![SOURCES_PRODUCER_JOB_ID.to_owned()];
        }
    }
}

fn producer_roots(jobs: &BTreeMap<String, Job>) -> Result<Vec<ProducerRoot>, RenderError> {
    let mut roots = Vec::new();
    for (id, job) in jobs {
        if job.producer_kind() != Some(ProducerKind::StateProducer) {
            continue;
        }
        let root = job
            .steps
            .iter()
            .find_map(|step| match (&step.role, &step.kind) {
                (Some(StepRole::MbxBundleKey), StepKind::Shell { env, .. }) => {
                    env.get(MBX_BUNDLE_ROOT_ENV).cloned()
                }
                _ => None,
            })
            .ok_or_else(|| invalid("producer_without_root"))?;
        let slug = RootSlug::from_root(&root).map_err(RenderError::Contract)?;
        if state_producer_job_id(&slug) != *id {
            return Err(invalid("producer_id_root_mismatch"));
        }
        roots.push(ProducerRoot {
            slug,
            producer_id: id.clone(),
            runs_on: job.runs_on.clone(),
        });
    }
    Ok(roots)
}

fn scope_prefix(slug: &RootSlug) -> String {
    StateScope {
        trust: StateTrust::Trusted,
        unit: StateUnit::Union,
        root: slug.clone(),
    }
    .prefix()
}

fn identity_prefix(slug: &RootSlug) -> String {
    format!(
        "{}{}-{}-",
        scope_prefix(slug),
        state_compat_plan_expr(slug.as_str()),
        state_snapshot_plan_expr(slug)
    )
}

fn seal_job(
    root: &ProducerRoot,
    needs: Vec<String>,
    pending: &[String],
    unit_bound: u64,
) -> Result<Job, RenderError> {
    let artifact = format!(
        "${{{{ needs.{}.outputs.{STATE_PRODUCER_ARTIFACT_OUTPUT} }}}}",
        root.producer_id
    );
    let mut download = action_step(
        "Download sealed payload",
        DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("artifact-ids".to_owned(), artifact.clone()),
            ("merge-multiple".to_owned(), "true".to_owned()),
            ("path".to_owned(), transport_dir()?),
        ]),
    )?;
    download.role = Some(StepRole::StateSealDownload);
    let subject = state_seal_subject_step(&scope_prefix(&root.slug), &identity_prefix(&root.slug))?;
    let admit = crate::state_seal::save::state_seal_admit_step(pending, unit_bound)?;
    let bind = state_seal_bind_step()?;
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
    attest.condition = Some(STATE_SEAL_ATTEST_CONDITION.to_owned());
    let mut save = action_step(
        "Save sealed payload",
        TOOLS_SAVE_USES,
        BTreeMap::from([
            (
                "key".to_owned(),
                velnor_actions_contract::workflow::state_seal::STATE_SEAL_KEY_EXPR.to_owned(),
            ),
            ("path".to_owned(), COMPLETE_STATE_TRANSPORT_EXPR.to_owned()),
        ]),
    )?;
    save.id = Some(StepId::StateSealSave);
    save.role = Some(StepRole::StateSealSave);
    save.condition = Some(STATE_SEAL_SAVE_CONDITION.to_owned());
    let retire =
        crate::state_seal::save::state_seal_retire_step(&scope_prefix(&root.slug), &artifact)?;
    Ok(Job {
        display_name: format!("{SEAL_DISPLAY_PREFIX}{}", root.slug),
        runs_on: root.runs_on.clone(),
        check_runner: None,
        timeout_minutes: JobTimeout::try_from(JOB_MINUTES).map_err(RenderError::Contract)?,
        needs,
        condition: Some(state_seal_condition(&root.producer_id)),
        permissions: Some(Permissions {
            contents: PermissionLevel::None,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::Write,
            actions: PermissionLevel::Write,
            packages: PermissionLevel::None,
            attest: PermissionLevel::Write,
        }),
        cache_access: CacheAccess::Writer(CacheWriter::StateSeal),
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: JobEnv::Unset,
        steps: vec![download, subject, admit, save, bind, attest, retire],
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

pub fn state_seal_bind_step() -> Result<Step, RenderError> {
    let script = format!(
        r#"{head}subject="$RUNNER_TEMP/{rel}"
mkdir -p "${{subject%/*}}"
rm -f "$subject"
refuse() {{ printf '%s\n' "::warning::cache_unavailable: state seal bind refused ($1)"; printf '%s\n' '{bound}=false' >> "$GITHUB_OUTPUT"; exit 0; }}
key="${{{key_env}-}}"
ref="${{GITHUB_REF-}}"
repo="${{GITHUB_REPOSITORY-}}"
[ -n "$key" ] && [ -n "$ref" ] && [ -n "$repo" ] || refuse input_missing
case "$key" in *[[:space:]]*) refuse key_whitespace ;; esac
number "$size" && [ "$size" -gt 0 ] || refuse payload_unreadable
bound_of "$size" || refuse bound_unavailable
command -v gh >/dev/null 2>&1 || refuse gh_missing
tries=
while :; do
  gh cache list -R "$repo" --key "$key" --ref "$ref" --limit {list_limit} --json {fields} --jq '{jq}' > "$d/list" || refuse list_failed
  hits=
  id=
  stored=
  while IFS='{tab}' read -r k i s; do
    [ "$k" = "$key" ] || continue
    case "$i" in ''|*[!0-9]*) refuse id_malformed ;; esac
    case "$s" in ''|*[!0-9]*) refuse size_malformed ;; esac
    hits="${{hits}}x"
    id=$i
    stored=$s
  done < "$d/list"
  [ -z "$hits" ] || break
  tries="${{tries}}x"
  [ "${{#tries}}" -lt {attempts} ] || break
  sleep {pause}
done
[ "$hits" = x ] || refuse "entries=${{#hits}}"
[ "$stored" -le "$bound" ] || refuse "size_out_of_bound stored=$stored bound=$bound"
printf '%s %s %s\n' "$key" "$id" "$stored" > "$subject"
printf '%s\n' '{bound}=true' >> "$GITHUB_OUTPUT"
printf 'velnor: state seal subject %s %s %s\n' "$key" "$id" "$stored"
"#,
        head = crate::state_seal::save::head("state-bind"),
        rel = STATE_SEAL_SUBJECT_REL,
        bound = BOUND_OUTPUT,
        key_env = STATE_SEAL_KEY_ENV,
        list_limit = STATE_SEAL_LIST_LIMIT,
        fields = STATE_SEAL_LIST_FIELDS,
        jq = STATE_SEAL_LIST_JQ,
        tab = STATE_SEAL_LIST_TAB,
        attempts = STATE_SEAL_BIND_ATTEMPTS,
        pause = STATE_SEAL_BIND_PAUSE_SECONDS,
    );
    let mut step = ambient_shell_step(
        "Bind seal subject",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        BTreeMap::from([
            (
                STATE_SEAL_KEY_ENV.to_owned(),
                STATE_SEAL_KEY_EXPR.to_owned(),
            ),
            ("GH_TOKEN".to_owned(), TOKEN_EXPR.to_owned()),
        ]),
    )?;
    step.id = Some(StepId::StateSealBind);
    step.role = Some(StepRole::StateSealBind);
    step.condition = Some(STATE_SEAL_BIND_CONDITION.to_owned());
    Ok(step)
}

pub fn state_seal_subject_step(scope: &str, prefix: &str) -> Result<Step, RenderError> {
    let stored = CacheFamily::MbxBundle.spec().ceiling.stored();
    let script = format!(
        r#"set -eu
{GUARD}
payload="{payload}"
d="$RUNNER_TEMP/{rel_dir}"
rm -rf "$d"
mkdir -p "$d"
refuse() {{ printf '%s\n' "::warning::cache_unavailable: state seal refused ($1)"; printf '%s\n' '{sealed}=false' >> "$GITHUB_OUTPUT"; exit 0; }}
s256() {{ sha256sum "$1" > "$d/sum" || return 1; h=; rest=; read -r h rest < "$d/sum" || [ -n "$h" ]; case "$h" in ''|*[!0-9a-f]*) return 1 ;; esac; [ "${{#h}}" -eq 64 ] || return 1; }}
prefix="${{{prefix_env}-}}"
scope="${{{scope_env}-}}"
[ -n "$prefix" ] && [ -n "$scope" ] || refuse prefix_missing
case "$prefix" in "$scope"?*) ;; *) refuse prefix_scope ;; esac
printf '%s\n' "${{prefix#"$scope"}}" | grep -Eq '^s256-[0-9a-f]{{64}}-s256-[0-9a-f]{{64}}-$' || refuse prefix_shape
if [ -L "$payload" ] || [ ! -f "$payload" ]; then refuse payload_not_regular; fi
size=; wc -c < "$payload" > "$d/size" || refuse size_unreadable
read -r size < "$d/size" || :
case "$size" in ''|*[!0-9]*) refuse size_unreadable ;; esac
if [ "$size" -eq 0 ]; then refuse payload_empty; fi
if [ "$size" -gt {stored} ]; then refuse "over_stored stored=$size ceiling={stored}"; fi
s256 "$payload" || refuse digest_unreadable
key="${{prefix}}s256-$h"
[ "${{#key}}" -le {max_key} ] || refuse key_too_long
printf '%s\n' '{sealed}=true' "key=$key" >> "$GITHUB_OUTPUT"
printf 'velnor: state seal subject %s\n' "$key"
"#,
        payload = transport_file()?,
        rel_dir = STATE_SEAL_SUBJECT_REL
            .rsplit_once('/')
            .map_or("", |(dir, _)| dir),
        sealed = SEALED_OUTPUT,
        prefix_env = STATE_SEAL_PREFIX_ENV,
        scope_env = velnor_actions_contract::workflow::state_seal::STATE_SEAL_SCOPE_ENV,
        max_key = velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES,
    );
    let mut step = shell_step(
        "Write seal subject",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        BTreeMap::from([
            (STATE_SEAL_PREFIX_ENV.to_owned(), prefix.to_owned()),
            (
                velnor_actions_contract::workflow::state_seal::STATE_SEAL_SCOPE_ENV.to_owned(),
                scope.to_owned(),
            ),
        ]),
    )?;
    step.id = Some(StepId::StateSealSubject);
    step.role = Some(StepRole::StateSealSubject);
    Ok(step)
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("state_seal:{problem}"))
}
