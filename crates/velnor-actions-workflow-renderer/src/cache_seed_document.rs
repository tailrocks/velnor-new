//! Generated `.github/workflows/cache-seed.yml`: the only dependency seed writer.
//!
//! Triggered by `workflow_run` after the CI workflow completes, gated per
//! job on a same-repository default-branch push, with its own concurrency
//! group (`cancel-in-progress: false`, so GitHub keeps one pending run and
//! the newest wins). It never holds CI's group and CI never waits for it.
//! Permissions: `contents: read` plus `actions: write` (cache deletes and
//! the triggering run's artifact download).

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, RunsOn, StepKind, StepRole};

use crate::{
    RenderError, document_steps::non_internal_step_to_yaml, document_steps::string_map_yaml,
    render::RenderedFile, render::fallback::render_checked_workflow, setup::MiseSetup, steps,
    yaml::Yaml,
};

/// Generated path of the Cache seed workflow.
pub const CACHE_SEED_WORKFLOW_PATH: &str = ".github/workflows/cache-seed.yml";
/// Display name of the Cache seed workflow.
pub const CACHE_SEED_WORKFLOW_NAME: &str = "Cache seed";
/// The seed's own concurrency group, keyed by the triggering branch.
pub const CACHE_SEED_CONCURRENCY_GROUP: &str =
    "velnor-cache-seed-${{ github.event.workflow_run.head_branch }}";

/// Inputs of one Cache seed workflow render.
#[derive(Debug)]
pub struct CacheSeedSpec<'a> {
    /// Display name of the CI workflow whose completion triggers the seed.
    pub ci_workflow_name: &'a str,
    /// One job per seeded root.
    pub jobs: BTreeMap<String, Job>,
    /// Pinned Mise setup of the default runner target.
    pub mise: &'a MiseSetup,
    /// Pinned checkout ref of the render context.
    pub checkout_uses: &'a str,
    /// Generator version for the file marker.
    pub generator_version: &'a str,
}

/// Finalize and render the Cache seed workflow.
///
/// Returns the file and the finalized jobs (for the cache budget check).
/// # Errors
///
/// Returns [`RenderError`] when a job, step, cache rule, or the document
/// fails validation.
pub fn render_cache_seed_workflow(
    spec: CacheSeedSpec<'_>,
) -> Result<(RenderedFile, BTreeMap<String, Job>), RenderError> {
    let mut jobs = spec.jobs;
    for (id, job) in &mut jobs {
        let target = crate::runs_on::target_for_runner(&job.runs_on).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("tools_cache_unsupported_target:{id}"))
        })?;
        // Seed jobs run on the CI default label, whose setup `mise` already is.
        let setup = spec.mise.clone();
        crate::cache_p08::ensure_tools_cache_v2(id, job, &setup, true, target, spec.checkout_uses)?;
        crate::cache_p08::check_no_legacy_rust_cache(id, job)?;
        crate::cache_p08::check_mbx_before_fetch(id, job)?;
        velnor_actions_contract::workflow::step_identity::validate_step_sequence(&job.steps, id)
            .map_err(RenderError::Contract)?;
    }
    crate::cache_steps::validate_mbx_lifecycles(&jobs)?;
    crate::cache_doctor::check(&jobs, crate::cache_doctor::DoctorScope::Seed)?;
    let mbx_jobs = crate::mbx_job_env::jobs_with_mbx(&jobs);
    let workflow_env = crate::toolchain_env::workflow_level_env();
    let mut rendered = Vec::with_capacity(jobs.len());
    for (id, job) in &jobs {
        rendered.push((
            id.clone(),
            job_to_yaml(id, job, &workflow_env, mbx_jobs.contains(id))?,
        ));
    }
    let document = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(CACHE_SEED_WORKFLOW_NAME)),
        ("on".to_owned(), trigger(spec.ci_workflow_name)?),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("env".to_owned(), string_map_yaml(&workflow_env)),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(CACHE_SEED_CONCURRENCY_GROUP)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(rendered)),
    ]);
    let text = annotate_trigger(&render_checked_workflow(
        CACHE_SEED_WORKFLOW_PATH,
        &document,
        spec.generator_version,
    )?)?;
    steps::scan_for_private_subcommands(&text)?;
    Ok((
        RenderedFile {
            path: CACHE_SEED_WORKFLOW_PATH.to_owned(),
            bytes: text,
        },
        jobs,
    ))
}

/// Mark the one `workflow_run` trigger line as reviewed for zizmor.
///
/// `workflow_run` is the documented way to run privileged work after a
/// workflow: every seed job is gated on a same-repository default-branch
/// push and checks out that push's tested commit, never fork code.
fn annotate_trigger(text: &str) -> Result<String, RenderError> {
    const LINE: &str = "\n  workflow_run:\n";
    if text.matches(LINE).count() != 1 {
        return Err(RenderError::InvalidWorkflow(
            "cache_seed_trigger_line".to_owned(),
        ));
    }
    Ok(text.replace(
        LINE,
        "\n  workflow_run: # zizmor: ignore[dangerous-triggers]\n",
    ))
}

fn trigger(ci_workflow_name: &str) -> Result<Yaml, RenderError> {
    if ci_workflow_name.trim().is_empty()
        || ci_workflow_name.contains("${{")
        || ci_workflow_name.chars().any(char::is_control)
    {
        return Err(RenderError::InvalidWorkflow(
            "cache_seed_bad_ci_name".to_owned(),
        ));
    }
    Ok(Yaml::Map(vec![(
        "workflow_run".to_owned(),
        Yaml::Map(vec![
            (
                "workflows".to_owned(),
                Yaml::Seq(vec![Yaml::str(ci_workflow_name.to_owned())]),
            ),
            ("types".to_owned(), Yaml::Seq(vec![Yaml::str("completed")])),
        ]),
    )]))
}

fn job_to_yaml(
    id: &str,
    job: &Job,
    workflow_env: &BTreeMap<String, String>,
    native_mbx: bool,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let runs_on = match RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)? {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(_) => {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_seed_requires_hosted_runner:{id}"
            )));
        }
    };
    let job_env = seed_job_env(job, native_mbx);
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), runs_on),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    let local = crate::document_env::exclude_inherited(&job_env, workflow_env);
    if !local.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&local)));
    }
    entries.push((
        "permissions".to_owned(),
        Yaml::Map(vec![
            ("contents".to_owned(), Yaml::str("read")),
            ("actions".to_owned(), Yaml::str("write")),
        ]),
    ));
    let condition = job
        .condition
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("cache_seed_job_ungated:{id}")))?;
    steps::scan_for_private_subcommands(condition)?;
    entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    let mut effective = workflow_env.clone();
    effective.extend(job_env);
    let mut rendered = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        rendered.push(non_internal_step_to_yaml(
            id,
            step,
            &effective,
            &job.runs_on,
        )?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered)));
    Ok(Yaml::Map(entries))
}

/// The crate jobs' job env: Rust toolchain plus auto-clean, the helper
/// acquisition binding, and the shared MBX policy.
fn seed_job_env(job: &Job, native_mbx: bool) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for step in &job.steps {
        let (StepKind::Shell { env: step_env, .. } | StepKind::Action { env: step_env, .. }) =
            &step.kind
        else {
            continue;
        };
        if let Some(toolchain) = step_env.get("RUSTUP_TOOLCHAIN") {
            env.insert("RUSTUP_TOOLCHAIN".to_owned(), toolchain.clone());
            let (key, value) = crate::toolchain_env::CARGO_AUTO_CLEAN_ENV;
            env.insert(key.to_owned(), value.to_owned());
        }
        if step.role == Some(StepRole::AcquireVelnor) {
            for key in [
                steps::ASSET_SHA_ENV,
                steps::ASSET_URL_ENV,
                steps::RELEASE_COMMIT_ENV,
            ] {
                if let Some(value) = step_env.get(key) {
                    env.insert(key.to_owned(), value.clone());
                }
            }
        }
    }
    if native_mbx {
        crate::mbx_job_env::extend_mbx_job_env(&mut env);
    }
    env
}
