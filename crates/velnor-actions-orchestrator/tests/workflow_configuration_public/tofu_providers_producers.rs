// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::workflow::cache_family::{
    TOFU_PROVIDERS_PROBE_MISS_CONDITION, TOFU_PROVIDERS_SAVE_CONDITION,
};
use velnor_actions_contract::workflow::{
    DEPENDENCY_SEED_JOB_CONDITION, ProducerKind, validate_tofu_providers_producer,
};
use velnor_actions_contract::{
    CacheMode, Job, PermissionLevel, Permissions, REQUIRED_JOB_ID, Step, StepKind, StepRole,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
use velnor_actions_workflow_renderer::tofu_cache::tofu_providers_save_step;

use super::support::{config_with_timeouts, rendered_job_block};
use super::tofu_providers_producer_condition::gate_or_survival;
use super::workflow_configuration_fixture;

const PRODUCER_PREFIX: &str = "providers-producer-";
pub(super) const TOFU_LIMIT: usize = 2;
const LOCK: &str = "provider \"registry.opentofu.org/hashicorp/random\" {\n  version = \"3.6.0\"\n  hashes = [\"h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"]\n}\n";
const MAIN: &str = "terraform {\n  required_providers {\n    random = {\n      source = \"hashicorp/random\"\n      version = \"3.6.0\"\n    }\n  }\n}\n";

pub(super) type Jobs = BTreeMap<String, Job>;

pub(super) fn tofu_repo(roots: &[&str]) -> Result<TempDir, Box<dyn Error>> {
    tofu_repo_with(roots, None)
}

pub(super) fn tofu_repo_with(
    roots: &[&str],
    crate_timeout: Option<u16>,
) -> Result<TempDir, Box<dyn Error>> {
    let quoted: Vec<String> = roots.iter().map(|root| format!("\"{root}\"")).collect();
    let config = format!(
        "{}max_parallel_jobs = {TOFU_LIMIT}\n\n[stacks.tofu]\nroots = [{}]\n",
        config_with_timeouts(15, crate_timeout)?,
        quoted.join(", ")
    );
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    for root in roots {
        fs::create_dir_all(repo.path().join(root))?;
        fs::write(repo.path().join(root).join("main.tf"), MAIN)?;
        fs::write(repo.path().join(root).join(".terraform.lock.hcl"), LOCK)?;
    }
    Ok(repo)
}

pub(super) fn finalized(repo: &TempDir) -> Result<Jobs, Box<dyn Error>> {
    Ok(finalized_jobs(&prepare(repo.path())?)?)
}

pub(super) fn producers(jobs: &Jobs) -> Vec<(&String, &Job)> {
    jobs.iter()
        .filter(|(_, job)| job.producer_kind() == Some(ProducerKind::TofuProviders))
        .collect()
}

pub(super) fn tofu_crate_jobs(jobs: &Jobs) -> Vec<(&String, &Job)> {
    jobs.iter()
        .filter(|(id, _)| id.starts_with("tofu-"))
        .collect()
}

fn step_of(job: &Job, role: StepRole) -> Result<&Step, Box<dyn Error>> {
    job.steps
        .iter()
        .find(|step| step.role == Some(role))
        .ok_or_else(|| format!("no {role:?} step").into())
}

fn action_input<'a>(step: &'a Step, name: &str) -> Result<&'a str, Box<dyn Error>> {
    match &step.kind {
        StepKind::Action { with, .. } => with
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("{} has no {name}", step.name).into()),
        _ => Err(format!("{} is not an action", step.name).into()),
    }
}

fn crate_provider_bindings(
    crates: &[(&String, &Job)],
) -> Result<BTreeSet<(String, String)>, Box<dyn Error>> {
    let mut bindings = BTreeSet::new();
    for (id, job) in crates {
        assert!(
            job.cache_mode
                .as_ref()
                .is_some_and(CacheMode::is_explicit_read),
            "{id}: {:?}",
            job.cache_mode
        );
        assert!(
            !job.steps
                .iter()
                .any(|step| step.role == Some(StepRole::TofuProvidersSave)),
            "{id} saves providers"
        );
        let restore = step_of(job, StepRole::TofuProvidersRestore)?;
        bindings.insert((
            action_input(restore, "cache-key")?.to_owned(),
            action_input(restore, "cache-path")?.to_owned(),
        ));
    }
    Ok(bindings)
}

fn producer_binding(job: &Job) -> Result<(String, String), Box<dyn Error>> {
    let probe = step_of(job, StepRole::TofuProvidersRestore)?;
    Ok((
        action_input(probe, "key")?.to_owned(),
        action_input(probe, "path")?.to_owned(),
    ))
}

pub(super) fn assert_producer_shape(
    id: &str,
    job: &Job,
    jobs: &Jobs,
) -> Result<(), Box<dyn Error>> {
    assert!(id.starts_with(PRODUCER_PREFIX), "{id}");
    validate_tofu_providers_producer(job, id)?;
    assert!(gate_or_survival(job), "{id}: {:?}", job.condition);
    assert!(job.needs.iter().any(|need| need == "plan"), "{id}");
    assert!(job.permissions.is_none() && job.cache_mode.is_none());
    let required = jobs.get(REQUIRED_JOB_ID).ok_or("no required job")?;
    assert!(
        !required.needs.contains(&id.to_owned()),
        "required waits for {id}"
    );
    assert!(
        jobs.values()
            .filter(|other| other.producer_kind().is_none())
            .all(|other| !other.needs.contains(&id.to_owned())),
        "a consumer waits for {id}"
    );
    let save = step_of(job, StepRole::TofuProvidersSave)?;
    assert_eq!(
        save.condition.as_deref(),
        Some(TOFU_PROVIDERS_SAVE_CONDITION)
    );
    let probe = step_of(job, StepRole::TofuProvidersRestore)?;
    assert_eq!(action_input(save, "key")?, action_input(probe, "key")?);
    assert_eq!(action_input(save, "path")?, action_input(probe, "path")?);
    assert_eq!(action_input(probe, "lookup-only")?, "true");
    let StepKind::Shell { run, env } = &step_of(job, StepRole::TofuProviderUse)?.kind else {
        return Err("fetch is not a shell step".into());
    };
    for arg in ["init", "-backend=false", "-lockfile=readonly"] {
        assert!(run.iter().any(|part| part == arg), "{id} fetch lacks {arg}");
    }
    assert_eq!(
        env.get("TF_PLUGIN_CACHE_DIR").map(String::as_str),
        Some(action_input(probe, "path")?)
    );
    assert!(env.keys().all(|name| !name.starts_with("VELNOR_")));
    for role in [
        StepRole::PreparePinnedTools,
        StepRole::TofuProvidersPrepare,
        StepRole::TofuProviderUse,
    ] {
        assert_eq!(
            step_of(job, role)?.condition.as_deref(),
            Some(TOFU_PROVIDERS_PROBE_MISS_CONDITION),
            "{id} {role:?}"
        );
    }
    Ok(())
}

#[test]
fn tofu_crate_jobs_only_read_and_one_push_only_producer_saves_each_provider_key()
-> Result<(), Box<dyn Error>> {
    let jobs = finalized(&tofu_repo(&["infra", "infra2"])?)?;
    let crates = tofu_crate_jobs(&jobs);
    assert_eq!(crates.len(), 2, "{:?}", jobs.keys().collect::<Vec<_>>());
    let crate_keys = crate_provider_bindings(&crates)?;
    assert_eq!(crate_keys.len(), 2);
    let found = producers(&jobs);
    assert_eq!(found.len(), 2);
    let produced = found
        .iter()
        .map(|(_, job)| producer_binding(job))
        .collect::<Result<BTreeSet<_>, _>>()?;
    assert_eq!(produced, crate_keys);
    for (id, job) in found {
        assert_producer_shape(id, job, &jobs)?;
    }
    Ok(())
}

#[test]
fn rendered_producer_is_continue_on_error_and_the_only_provider_writer()
-> Result<(), Box<dyn Error>> {
    let repo = tofu_repo(&["infra"])?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("CI workflow is missing")?;
    let jobs = finalized(&repo)?;
    let (id, _) = *producers(&jobs).first().ok_or("no producer")?;
    let block = rendered_job_block(ci, id)?;
    assert!(block.contains("    continue-on-error: true\n"), "{block}");
    assert!(block.contains(&format!("    if: {DEPENDENCY_SEED_JOB_CONDITION}\n")));
    assert_eq!(block.matches("name: Save Tofu providers").count(), 1);
    assert_eq!(ci.matches("name: Save Tofu providers").count(), 1);
    assert!(!block.contains("permissions:"));
    Ok(())
}

type Apply = fn(&mut Job, &Step);

struct Mutation {
    name: &'static str,
    apply: Apply,
}

fn edit_step(job: &mut Job, role: StepRole, edit: impl FnOnce(&mut Step)) {
    if let Some(step) = job.steps.iter_mut().find(|step| step.role == Some(role)) {
        edit(step);
    }
}

fn mutation(name: &'static str, apply: Apply) -> Mutation {
    Mutation { name, apply }
}

fn job_mutations() -> Vec<Mutation> {
    vec![
        mutation("drops_needs_plan", |job, _| job.needs.clear()),
        mutation("foreign_need", |job, _| {
            job.needs.push("rust-app".to_owned());
        }),
        mutation("untrusted_condition", |job, _| {
            job.condition = Some("github.event_name == 'push'".to_owned());
        }),
        mutation("contents_write", |job, _| {
            job.permissions = Some(Permissions {
                contents: PermissionLevel::Write,
                ..Permissions::default()
            });
        }),
        mutation("environment", |job, _| {
            job.environment = Some("production".to_owned());
        }),
        mutation("second_save", |job, save| job.steps.push(save.clone())),
        mutation("no_save", |job, _| {
            job.steps
                .retain(|step| step.role != Some(StepRole::TofuProvidersSave));
        }),
        mutation("save_before_fetch", |job, _| {
            let last = job.steps.len() - 1;
            job.steps.swap(last - 1, last);
        }),
        mutation("foreign_step", |job, _| {
            let first = job.steps[0].clone();
            job.steps.insert(1, first);
        }),
    ]
}

fn step_mutations() -> Vec<Mutation> {
    vec![
        mutation("save_without_miss_gate", |job, _| {
            edit_step(job, StepRole::TofuProvidersSave, |step| {
                step.condition = Some("success()".to_owned());
            });
        }),
        mutation("save_ungated", |job, _| {
            edit_step(job, StepRole::TofuProvidersSave, |step| {
                step.condition = None;
            });
        }),
        mutation("fetch_ungated", |job, _| {
            edit_step(job, StepRole::TofuProviderUse, |step| step.condition = None);
        }),
        mutation("prepare_ungated", |job, _| {
            edit_step(job, StepRole::TofuProvidersPrepare, |step| {
                step.condition = None;
            });
        }),
        mutation("prepare_script_weakened", |job, _| {
            edit_step(job, StepRole::TofuProvidersPrepare, |step| {
                if let StepKind::Shell { run, .. } = &mut step.kind {
                    "rm -rf -- \"$TOFU_PROVIDER_CACHE_PATH\"".clone_into(&mut run[2]);
                }
            });
        }),
        mutation("prepare_path_rebound", |job, _| {
            edit_step(job, StepRole::TofuProvidersPrepare, |step| {
                if let StepKind::Shell { env, .. } = &mut step.kind {
                    env.insert("TOFU_PROVIDER_CACHE_PATH".to_owned(), "/".to_owned());
                }
            });
        }),
        mutation("prepare_extra_env", |job, _| {
            edit_step(job, StepRole::TofuProvidersPrepare, |step| {
                if let StepKind::Shell { env, .. } = &mut step.kind {
                    env.insert("IFS".to_owned(), "x".to_owned());
                }
            });
        }),
        mutation("prepare_trailing_command", |job, _| {
            edit_step(job, StepRole::TofuProvidersPrepare, |step| {
                if let StepKind::Shell { run, .. } = &mut step.kind {
                    run[2].push_str("; touch /tmp/x");
                }
            });
        }),
        mutation("prepare_not_inline_shell", |job, _| {
            edit_step(job, StepRole::TofuProvidersPrepare, |step| {
                if let StepKind::Shell { run, .. } = &mut step.kind {
                    run.insert(0, "env".to_owned());
                }
            });
        }),
        mutation("install_ungated", |job, _| {
            edit_step(job, StepRole::PreparePinnedTools, |step| {
                step.condition = None;
            });
        }),
        mutation("probe_not_lookup_only", |job, _| {
            edit_step(job, StepRole::TofuProvidersRestore, |step| {
                if let StepKind::Action { with, .. } = &mut step.kind {
                    with.remove("lookup-only");
                }
            });
        }),
    ]
}

#[test]
fn producer_validator_rejects_each_mutation_and_crate_jobs() -> Result<(), Box<dyn Error>> {
    let jobs = finalized(&tofu_repo(&["infra"])?)?;
    let (id, producer) = *producers(&jobs).first().ok_or("no producer")?;
    validate_tofu_providers_producer(producer, id)?;
    let save = step_of(producer, StepRole::TofuProvidersSave)?.clone();
    let mut accepted = Vec::new();
    for Mutation { name, apply } in job_mutations().into_iter().chain(step_mutations()) {
        let mut job = producer.clone();
        apply(&mut job, &save);
        if validate_tofu_providers_producer(&job, id).is_ok() {
            accepted.push(name);
        }
    }
    assert_eq!(accepted, Vec::<&str>::new());
    assert!(validate_tofu_providers_producer(producer, "tofu-infra").is_err());
    let (crate_id, crate_job) = *tofu_crate_jobs(&jobs).first().ok_or("no crate job")?;
    assert!(validate_tofu_providers_producer(crate_job, crate_id).is_err());
    let mut writer = crate_job.clone();
    writer.steps.push(save);
    assert!(validate_tofu_providers_producer(&writer, crate_id).is_err());
    Ok(())
}

#[test]
fn render_rejects_a_provider_save_in_a_crate_job() -> Result<(), Box<dyn Error>> {
    let repo = tofu_repo(&["infra"])?;
    let mut prepared = prepare(repo.path())?;
    let id = prepared
        .workflow
        .ir
        .jobs
        .keys()
        .find(|id| id.starts_with("tofu-"))
        .cloned()
        .ok_or("no tofu job")?;
    let job = prepared
        .workflow
        .ir
        .jobs
        .get_mut(&id)
        .ok_or("tofu job vanished")?;
    let restore = step_of(job, StepRole::TofuProvidersRestore)?;
    let save = tofu_providers_save_step(
        action_input(restore, "cache-key")?,
        action_input(restore, "cache-path")?,
    )?;
    job.steps.push(save);
    let error = finalized_jobs(&prepared)
        .err()
        .ok_or("a provider save in a crate job rendered")?;
    assert!(
        error
            .to_string()
            .contains(&format!("mixed_or_invalid_producer:{id}")),
        "{error}"
    );
    Ok(())
}
