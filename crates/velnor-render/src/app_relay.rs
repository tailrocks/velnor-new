// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::HelperRelease;

use crate::helper_integrity::INVOCATION_PREFIX;
use crate::private_steps::CHECK_STEP;
use crate::rules::{RenderedJob, RenderedStep, SETUP_VELNOR_ACTION, Value};
use crate::steps::{DOWNLOAD_ARTIFACT, UPLOAD_ARTIFACT};

pub(crate) const RELAY_HELPER: (u64, u64, u64) = (0, 1, 117);
const KEY_VARIABLE: &str = "VELNOR_APP_PRIVATE_KEY";
const JOB_VARIABLE: &str = "VELNOR_APP_JOB";
const PREFIXES: [&str; 2] = ["velnor-ci ", INVOCATION_PREFIX];
const INSTALL_TOOLS: &str = "eval \"$VELNOR_HELPER_INSTALL_TOOLS\"";
const INSTALL_LAUNCHER: &str = "eval \"$VELNOR_HELPER_INSTALL_LAUNCHER\"";
const API_ONLY_COMMANDS: [&str; 5] = [
    "app-token",
    "revoke-token",
    "private-token-allowed",
    "release-gate",
    "release",
];
const UPLOAD_VERSION: &str = "v7.0.2";
const RELAY_HOST: &str = "ubuntu-latest";
const RELAY_EXTRA_MINUTES: u16 = 5;
const LONGEST_WAIT_SECONDS: u32 = 3600;

pub(crate) fn holds_key(step: &RenderedStep) -> bool {
    step.env.iter().any(|(name, _)| name == KEY_VARIABLE)
        || step.with.iter().any(|(name, _)| name == "private-key")
}

fn mint_command(step: &RenderedStep) -> Option<(&'static str, &str)> {
    let run = step.run.as_deref()?;
    PREFIXES.iter().find_map(|prefix| {
        let rest = run.strip_prefix(prefix)?.strip_prefix("app-token ")?;
        (!rest.starts_with("request ")
            && !rest.starts_with("relay ")
            && !rest.starts_with("await "))
        .then_some((*prefix, rest))
    })
}

pub(crate) fn runs_repository_code(step: &RenderedStep) -> bool {
    if let Some(uses) = &step.uses {
        return !(uses.starts_with(&format!("{SETUP_VELNOR_ACTION}@"))
            || uses.starts_with(UPLOAD_ARTIFACT.split('@').next().unwrap_or_default())
            || uses.starts_with(DOWNLOAD_ARTIFACT.split('@').next().unwrap_or_default()));
    }
    step.run.as_deref().is_some_and(|run| {
        !(run == INSTALL_TOOLS || run == INSTALL_LAUNCHER || api_only_command(run))
    })
}

fn api_only_command(run: &str) -> bool {
    PREFIXES.iter().any(|prefix| {
        run.strip_prefix(prefix)
            .and_then(|command| command.split_whitespace().next())
            .is_some_and(|name| API_ONLY_COMMANDS.contains(&name))
    })
}

pub(crate) fn job_runs_repository_code(job: &RenderedJob) -> bool {
    job.steps.iter().any(runs_repository_code)
}

pub(crate) fn job_holds_key(job: &RenderedJob) -> bool {
    job.steps.iter().any(holds_key)
}

fn install_bundle(steps: &[RenderedStep]) -> Option<std::ops::RangeInclusive<usize>> {
    let setup = steps.iter().position(|step| {
        step.uses
            .as_deref()
            .is_some_and(|uses| uses.starts_with(&format!("{SETUP_VELNOR_ACTION}@")))
    })?;
    let runs = |index: usize, text: &str| {
        steps
            .get(index)
            .and_then(|step| step.run.as_deref())
            .is_some_and(|run| run == text)
    };
    let start = if setup > 0 && runs(setup - 1, INSTALL_TOOLS) {
        setup - 1
    } else {
        setup
    };
    let end = if runs(setup + 1, INSTALL_LAUNCHER) {
        setup + 1
    } else {
        setup
    };
    Some(start..=end)
}

fn gate(condition: Option<&str>, steps: &[RenderedStep]) -> Option<(String, RenderedStep)> {
    let reference = format!("steps.{CHECK_STEP}.");
    if !condition?.contains(&reference) {
        return None;
    }
    let check = steps
        .iter()
        .find(|step| step.id.as_deref() == Some(CHECK_STEP))?;
    Some((
        format!(
            "${{{{ {} }}}}",
            crate::private_steps::token_allowed(CHECK_STEP)
        ),
        check.clone(),
    ))
}

fn identity(job: &RenderedJob, index: usize, count: usize) -> Value {
    let base = if count == 1 {
        job.id.clone()
    } else {
        format!("{}-{}", job.id, index + 1)
    };
    if job.matrix.is_some() {
        Value::Expr(format!("{base}-${{{{ strategy.job-index }}}}"))
    } else {
        Value::Text(base)
    }
}

fn command(prefix: &str, rest: &str) -> String {
    format!("{prefix}app-token {rest}")
}

fn api_env() -> Vec<(String, Value)> {
    vec![
        (
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        ),
        (
            "VELNOR_API_URL".to_owned(),
            Value::Expr("${{ github.api_url }}".to_owned()),
        ),
    ]
}

fn upload(id: &str, name: &str) -> RenderedStep {
    RenderedStep {
        name: format!("Upload the {name}"),
        uses: Some(UPLOAD_ARTIFACT.to_owned()),
        uses_comment: Some(UPLOAD_VERSION.to_owned()),
        with: vec![
            (
                "name".to_owned(),
                Value::Expr(format!("${{{{ steps.{id}.outputs.artifact }}}}")),
            ),
            (
                "path".to_owned(),
                Value::Expr(format!("${{{{ steps.{id}.outputs.path }}}}")),
            ),
            (
                "if-no-files-found".to_owned(),
                Value::Text("error".to_owned()),
            ),
            ("retention-days".to_owned(), Value::Int(1)),
        ],
        ..RenderedStep::default()
    }
}

fn wait_seconds(job: &RenderedJob) -> u32 {
    (u32::from(job.timeout_minutes) * 60).clamp(60, LONGEST_WAIT_SECONDS)
}

pub(crate) fn split(job: RenderedJob, helper: Option<&HelperRelease>) -> Vec<RenderedJob> {
    let keys: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| mint_command(step).is_some() && holds_key(step))
        .map(|(index, _)| index)
        .collect();
    let Some(bundle) = install_bundle(&job.steps) else {
        return vec![job];
    };
    if keys.is_empty()
        || !helper.is_some_and(|release| release.version().at_least(RELAY_HELPER))
        || !job_runs_repository_code(&job)
    {
        return vec![job];
    }
    let mut building = job.clone();
    let mut relays = Vec::new();
    let mut inserted = 0;
    for (number, key) in keys.iter().enumerate() {
        let who = identity(&job, number, keys.len());
        let (request, relay) = handshake(&job, &job.steps[*key], &who, &bundle);
        let (request_steps, await_step) = request;
        let at = key + inserted;
        building.steps[at] = await_step;
        let anchor = bundle.end() + 1 + number * 2;
        for (offset, step) in request_steps.into_iter().enumerate() {
            building.steps.insert(anchor + offset, step);
        }
        inserted += 2;
        relays.push(relay);
    }
    add_actions_read(&mut building);
    std::iter::once(building).chain(relays).collect()
}

fn add_actions_read(job: &mut RenderedJob) {
    if job.permissions.iter().all(|(scope, _)| scope != "actions") {
        if job.permissions.is_empty() && !job.no_permissions {
            job.permissions
                .push(("contents".to_owned(), "read".to_owned()));
        }
        job.permissions
            .push(("actions".to_owned(), "read".to_owned()));
        job.no_permissions = false;
    }
}

type Handshake = ((Vec<RenderedStep>, RenderedStep), RenderedJob);

fn handshake(
    job: &RenderedJob,
    key: &RenderedStep,
    who: &Value,
    bundle: &std::ops::RangeInclusive<usize>,
) -> Handshake {
    let (prefix, rest) = mint_command(key).unwrap_or_default();
    let step_id = key.id.clone().unwrap_or_else(|| "app".to_owned());
    let request_id = format!("{step_id}-request");
    let job_env = vec![(JOB_VARIABLE.to_owned(), who.clone())];
    let request = RenderedStep {
        id: Some(request_id.clone()),
        name: "Publish the public key for the App token".to_owned(),
        env: job_env.clone(),
        run: Some(command(
            prefix,
            &format!("request --job=\"${JOB_VARIABLE}\""),
        )),
        ..RenderedStep::default()
    };
    let mut await_env = job_env.clone();
    await_env.extend(api_env());
    let received = RenderedStep {
        env: await_env,
        with: Vec::new(),
        uses: None,
        uses_comment: None,
        run: Some(command(prefix, &format!("await --job=\"${JOB_VARIABLE}\""))),
        ..key.clone()
    };
    let relay = relay_job(job, key, (&step_id, prefix, rest), &job_env, bundle);
    (
        (vec![request, upload(&request_id, "public key")], received),
        relay,
    )
}

fn relay_job(
    job: &RenderedJob,
    key: &RenderedStep,
    (step_id, prefix, rest): (&str, &str, &str),
    job_env: &[(String, Value)],
    bundle: &std::ops::RangeInclusive<usize>,
) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = job.steps[bundle.clone()].to_vec();
    let (guard, gate_step) = gate(key.condition.as_deref(), &job.steps).unzip();
    steps.extend(gate_step);
    let mut env = key.env.clone();
    env.extend(
        api_env()
            .into_iter()
            .filter(|(name, _)| env_lacks(&key.env, name)),
    );
    env.extend(job_env.iter().cloned());
    let relay_id = format!("{step_id}-relay");
    steps.push(RenderedStep {
        id: Some(relay_id.clone()),
        name: "Seal the App token to the job's public key".to_owned(),
        condition: guard.clone(),
        env,
        run: Some(command(
            prefix,
            &format!(
                "relay --job=\"${JOB_VARIABLE}\" {rest} --timeout-seconds={}",
                wait_seconds(job)
            ),
        )),
        ..RenderedStep::default()
    });
    steps.push(RenderedStep {
        condition: guard,
        ..upload(&relay_id, "sealed token")
    });
    let mut permissions: Vec<(String, String)> = job
        .permissions
        .iter()
        .filter(|(scope, _)| scope != "id-token" && scope != "actions")
        .cloned()
        .collect();
    permissions.push(("actions".to_owned(), "read".to_owned()));
    RenderedJob {
        id: format!("app-token-{}", relay_suffix(job, step_id)),
        name: None,
        check_name: None,
        comment: None,
        needs: job.needs.clone(),
        condition: job.condition.clone(),
        environment: job.environment.clone(),
        environment_note: job.environment_note.clone(),
        permissions,
        matrix: job.matrix.clone(),
        runs_on: RELAY_HOST.to_owned(),
        timeout_minutes: job.timeout_minutes.saturating_add(RELAY_EXTRA_MINUTES),
        cache_mode: job.cache_mode.clone(),
        steps,
        ..RenderedJob::default()
    }
}

fn env_lacks(env: &[(String, Value)], name: &str) -> bool {
    env.iter().all(|(existing, _)| existing != name)
}

fn relay_suffix(job: &RenderedJob, step_id: &str) -> String {
    if step_id == "app" || step_id == crate::private_steps::TOKEN_STEP {
        job.id.clone()
    } else {
        format!("{}-{step_id}", job.id)
    }
}

fn setup_version(job: &RenderedJob) -> Option<(u64, u64, u64)> {
    let step = job.steps.iter().find(|step| {
        step.uses
            .as_deref()
            .is_some_and(|uses| uses.starts_with(&format!("{SETUP_VELNOR_ACTION}@")))
    })?;
    let Some((_, Value::Text(version))) = step.with.iter().find(|(name, _)| name == "version")
    else {
        return None;
    };
    let mut parts = version.split('.').map(str::parse::<u64>);
    Some((
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ))
}

pub(crate) fn isolates_key(job: &RenderedJob) -> bool {
    setup_version(job).is_some_and(|version| version >= RELAY_HELPER)
        && job_holds_key(job)
        && job_runs_repository_code(job)
}
