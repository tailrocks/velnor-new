// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_JOB_ID;
use velnor_actions_contract::{
    Job, PermissionLevel, Permissions, REQUIRED_JOB_ID, Step, StepId, StepKind, StepRole,
    WorkflowPolicy,
};

use velnor_actions_orchestrator::prepare;

use super::one_job_per_crate::{
    Rendered, crate_jobs, is_emit, mbx_repo_for, mbx_repo_with_crate_timeout, render,
    rust_fixtures, tofu_fixture, tofu_fixture_for,
};
use super::tools_producers::is_upload;

const BINARY: &str = "$RUNNER_TEMP/velnor/ledger-cli/velnor-actions";
const ARTIFACT_NAME_EXPR: &str = "${{ needs.ledger-cli.outputs.artifact_name }}";

pub(super) fn velnor_fixtures() -> Result<Vec<Rendered>, Box<dyn Error>> {
    let mut fixtures: Vec<Rendered> = rust_fixtures()?
        .into_iter()
        .filter(|fixture| fixture.policy == WorkflowPolicy::VelnorRepositoryV1)
        .collect();
    fixtures.push(tofu_fixture_for(WorkflowPolicy::VelnorRepositoryV1)?);
    Ok(fixtures)
}

fn named<'a>(job: &'a Job, name: &str) -> Vec<(usize, &'a Step)> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.name == name)
        .collect()
}

fn one<'a>(job: &'a Job, name: &str) -> Result<(usize, &'a Step), Box<dyn Error>> {
    let found = named(job, name);
    match found[..] {
        [single] => Ok(single),
        _ => Err(format!("{name}: expected one step, found {}", found.len()).into()),
    }
}

fn with<'a>(step: &'a Step, key: &str) -> Option<&'a str> {
    match &step.kind {
        StepKind::Action { with, .. } => with.get(key).map(String::as_str),
        _ => None,
    }
}

#[test]
fn velnor_repository_has_one_ledger_cli_job() -> Result<(), Box<dyn Error>> {
    for Rendered { name, jobs, .. } in velnor_fixtures()? {
        let builders: Vec<&String> = jobs
            .iter()
            .filter(|(_, job)| job.steps.iter().any(|step| step.name == "Build ledger CLI"))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(builders, [LEDGER_CLI_JOB_ID], "{name}");
        let job = &jobs[LEDGER_CLI_JOB_ID];
        assert!(
            job.needs.is_empty(),
            "{name}: ledger-cli needs {:?}",
            job.needs
        );
        let read_only = Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::None,
            packages: PermissionLevel::None,
        };
        assert_eq!(job.permissions.as_ref(), Some(&read_only), "{name}");
        assert_eq!(job.environment, None, "{name}");
        let rendered = format!("{job:?}");
        assert!(!rendered.contains("secrets."), "{name}: secret reference");
        assert!(
            !rendered.contains("github.token"),
            "{name}: token reference"
        );
        assert_eq!(
            job.outputs
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["artifact_name", "sha256", "source_sha"]),
            "{name}"
        );
        assert!(
            job.outputs["artifact_name"].contains("github.run_id"),
            "{name}: artifact name is not run-keyed"
        );
        let uploads: Vec<&Step> = job.steps.iter().filter(|step| is_upload(step)).collect();
        assert_eq!(uploads.len(), 1, "{name}");
        let upload = uploads[0];
        assert_eq!(
            with(upload, "name"),
            Some(job.outputs["artifact_name"].as_str())
        );
        assert!(
            matches!(with(upload, "overwrite"), None | Some("false")),
            "{name}: upload overwrites"
        );
        assert_eq!(with(upload, "if-no-files-found"), Some("error"), "{name}");
        let retention = |step: &Step| with(step, "retention-days")?.parse::<u32>().ok();
        let (_, plan_upload) = one(&jobs["plan"], "Publish plan")?;
        let ledger_days = retention(upload).ok_or("ledger upload retention")?;
        assert!(
            ledger_days >= 30 && Some(ledger_days) >= retention(plan_upload),
            "{name}: ledger retention {ledger_days} days does not cover the re-run window"
        );
    }
    Ok(())
}

#[test]
fn ledger_cli_build_budget_ignores_a_one_minute_crate_timeout() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo_with_crate_timeout(false, WorkflowPolicy::VelnorRepositoryV1, Some(1))?;
    let Rendered { jobs, .. } = render(
        "one-minute-crates",
        &repo,
        WorkflowPolicy::VelnorRepositoryV1,
    )?;
    let budget = jobs[LEDGER_CLI_JOB_ID].timeout_minutes.minutes();
    assert!(
        budget >= 10,
        "ledger-cli budget {budget} minutes cannot hold the measured 105 s build"
    );
    let crates = crate_jobs(&jobs);
    assert!(!crates.is_empty());
    for (id, job) in crates {
        assert_eq!(job.timeout_minutes.minutes(), 1, "{id} timeout widened");
    }
    Ok(())
}

#[test]
fn crate_and_tofu_jobs_fetch_verify_then_emit() -> Result<(), Box<dyn Error>> {
    for Rendered { name, jobs, .. } in velnor_fixtures()? {
        let required = jobs.get(REQUIRED_JOB_ID).ok_or("missing required job")?;
        assert!(
            !required.needs.iter().any(|need| need == LEDGER_CLI_JOB_ID),
            "{name}: required waits on ledger-cli"
        );
        let crates = crate_jobs(&jobs);
        assert!(!crates.is_empty(), "{name}: no crate jobs");
        for (id, job) in crates {
            assert!(required.needs.contains(id), "{name}: required skips {id}");
            assert_eq!(
                job.needs
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from(["plan", LEDGER_CLI_JOB_ID]),
                "{name}: {id} needs"
            );
            for step in &job.steps {
                if let StepKind::Shell { run, .. } = &step.kind {
                    let command = run.join(" ");
                    assert!(
                        !command.contains("velnor-actions-cli")
                            && !command.contains("cargo run")
                            && !command.contains("cargo build"),
                        "{name}: {id} builds the CLI in {}",
                        step.name
                    );
                }
            }
            let last_task = job
                .steps
                .iter()
                .rposition(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
                .ok_or("no task step")?;
            let (download_at, download) = one(job, "Download ledger CLI")?;
            let (verify_at, verify) = one(job, "Verify ledger CLI")?;
            let (emit_at, emit) = one(job, "Emit group ledger")?;
            let upload_at = job
                .steps
                .iter()
                .position(|step| step.name == "Upload crate reports")
                .ok_or("no report upload")?;
            assert!(
                last_task < download_at
                    && download_at < verify_at
                    && verify_at < emit_at
                    && emit_at < upload_at,
                "{name}: {id} step order"
            );
            assert!(is_emit(emit));
            assert_eq!(
                with(download, "name"),
                Some(ARTIFACT_NAME_EXPR),
                "{name}: {id}"
            );
            let verify_id = verify.id.ok_or("verify has no id")?;
            assert_eq!(verify_id, StepId::LedgerCliVerify);
            assert_eq!(verify.condition.as_deref(), Some("always()"));
            let expected = format!(
                "always() && steps.{}.outcome == 'success'",
                verify_id.as_str()
            );
            assert_eq!(emit.condition.as_deref(), Some(expected.as_str()));
            let StepKind::Shell { run, .. } = &emit.kind else {
                return Err(format!("{name}: {id} emit is not a shell step").into());
            };
            assert_eq!(run.first().map(String::as_str), Some(BINARY));
        }
    }
    Ok(())
}

#[test]
fn consumer_policy_has_no_ledger_cli_job() -> Result<(), Box<dyn Error>> {
    let mut fixtures: Vec<Rendered> = rust_fixtures()?
        .into_iter()
        .filter(|fixture| fixture.policy == WorkflowPolicy::ConsumerV1)
        .collect();
    fixtures.push(tofu_fixture()?);
    for Rendered { name, jobs, .. } in fixtures {
        assert!(!jobs.contains_key(LEDGER_CLI_JOB_ID), "{name}");
        for (id, job) in crate_jobs(&jobs) {
            assert!(
                !job.needs.iter().any(|need| need == LEDGER_CLI_JOB_ID),
                "{name}: {id} needs ledger-cli"
            );
            for hidden in [
                "Download ledger CLI",
                "Verify ledger CLI",
                "Build ledger CLI",
            ] {
                assert!(named(job, hidden).is_empty(), "{name}: {id} has {hidden}");
            }
            let (_, emit) = one(job, "Emit group ledger")?;
            assert!(
                matches!(emit.kind, StepKind::Internal { .. }),
                "{name}: {id}"
            );
        }
    }
    Ok(())
}

fn role_at(job: &Job, role: StepRole) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(role))
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn ledger_cli_restores_sources_read_only_before_an_offline_build() -> Result<(), Box<dyn Error>> {
    for Rendered { name, jobs, .. } in velnor_fixtures()? {
        let job = &jobs[LEDGER_CLI_JOB_ID];
        let [keys] = role_at(job, StepRole::CacheKeys)[..] else {
            return Err(format!("{name}: expected one cache-keys step").into());
        };
        let [restore] = role_at(job, StepRole::CargoSourcesRestore)[..] else {
            return Err(format!("{name}: expected one sources restore").into());
        };
        let fetches = role_at(job, StepRole::CargoSourcesFetch);
        let [build] = role_at(job, StepRole::LedgerCliBuild)[..] else {
            return Err(format!("{name}: expected one build").into());
        };
        assert!(
            fetches
                .first()
                .is_some_and(|fetch| keys < restore && restore < *fetch)
                && fetches.iter().all(|fetch| *fetch < build),
            "{name}: order keys {keys} restore {restore} fetch {fetches:?} build {build}"
        );
        assert!(
            role_at(job, StepRole::CargoSourcesSave).is_empty()
                && !job.steps.iter().any(|step| matches!(&step.kind,
                    StepKind::Action { uses, .. } if uses.starts_with("actions/cache/save@"))),
            "{name}: ledger-cli saves sources"
        );
        assert_eq!(
            with(&job.steps[restore], "key"),
            Some("${{steps.cache-keys.outputs.sources_key}}"),
            "{name}"
        );
        let StepKind::Shell { run, env, .. } = &job.steps[build].kind else {
            return Err(format!("{name}: build is not a shell step").into());
        };
        assert!(
            run.iter().any(|arg| arg == "--locked") && run.iter().any(|arg| arg == "--offline"),
            "{name}: build is not locked and offline: {run:?}"
        );
        assert_eq!(
            env.get("RUSTUP_AUTO_INSTALL").map(String::as_str),
            Some("0"),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn velnor_repository_without_root_cargo_lock_cannot_build_the_ledger_cli()
-> Result<(), Box<dyn Error>> {
    let repo = mbx_repo_for(false, WorkflowPolicy::VelnorRepositoryV1)?;
    fs::remove_file(repo.path().join("Cargo.lock"))?;
    let error = prepare(repo.path())
        .err()
        .ok_or("prepared without a root Cargo.lock")?
        .to_string();
    assert!(
        error.contains("ledger_cli_requires_root_cargo_lock"),
        "unexpected error: {error}"
    );
    Ok(())
}
