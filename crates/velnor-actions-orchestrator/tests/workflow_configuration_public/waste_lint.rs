// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::ledger_cli::LEDGER_CLI_JOB_ID;
use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, HomeKey, Job, JobEnv, PLAN_JOB_ID, REQUIRED_JOB_ID, StepKind, StepRole,
    WorkflowPolicy,
};
use velnor_actions_orchestrator::{
    ExecWord, WasteFinding, finalized_jobs, prepare, render_staged_tree, waste_findings,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;
use super::support::rendered_job_block;
use super::tools_producers::{POLICIES, finalized};
use super::tools_producers_donors::{SCHEDULED_SUITES_CONFIG, prepared_for};
use super::waste_oracle::{
    self as oracle, AUTO_INSTALL_ENV, Files, JobUse, files_of, files_of_repo, job_env_entries,
    job_ids,
};
use super::workflow_configuration_fixture;

const CI_PATH: &str = ".github/workflows/ci.yml";
const FRESHNESS_PATH: &str = ".github/workflows/freshness.yml";
const UNCOVERED_WORKFLOWS: [&str; 6] = [
    "job-image-candidate.yml",
    "product-release-binary.yml",
    "product-release-generator.yml",
    "product-release-images.yml",
    "product-release.yml",
    "qualification.yml",
];

fn auto_install_off(entries: &[(String, String)]) -> bool {
    entries
        .iter()
        .any(|(key, value)| key == AUTO_INSTALL_ENV && value == "\"0\"")
}

fn bound_ids(jobs: &BTreeMap<String, Job>) -> Vec<&str> {
    jobs.iter()
        .filter(|(_, job)| job.env != JobEnv::Unset)
        .map(|(id, _)| id.as_str())
        .collect()
}

#[test]
fn rust_bearing_jobs_render_auto_install_off_at_job_level() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let prepared = prepare(repo.path())?;
        let jobs = finalized_jobs(&prepared)?;
        let tree = render_staged_tree(&prepared)?;
        let ci = tree.get(WORKFLOW_PATH).ok_or("CI workflow is missing")?;
        let bound = bound_ids(&jobs);
        assert!(bound.len() >= 3, "{policy:?}: only {bound:?} are bound");
        for id in job_ids(ci) {
            let entries = job_env_entries(ci, rendered_job_block(ci, id)?)?;
            assert_eq!(
                auto_install_off(&entries),
                bound.contains(&id),
                "{policy:?} {id}: rendered job env {entries:?}"
            );
        }
        if policy == WorkflowPolicy::VelnorRepositoryV1 {
            assert!(bound.contains(&PLAN_JOB_ID), "{bound:?}");
            assert!(bound.contains(&LEDGER_CLI_JOB_ID), "{bound:?}");
            assert!(
                bound.iter().any(|id| id.starts_with(CRATE_JOB_ID_PREFIX)),
                "{bound:?}"
            );
        }
    }
    Ok(())
}

fn home_env(step: &mut velnor_actions_contract::Step) -> Option<&mut BTreeMap<String, String>> {
    match &mut step.kind {
        StepKind::Shell { env, .. } => Some(env),
        _ => None,
    }
}

fn toolchain_step(jobs: &BTreeMap<String, Job>) -> Result<(String, usize), Box<dyn Error>> {
    jobs.iter()
        .filter(|(_, job)| job.env != JobEnv::Unset)
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .position(|step| {
                    step.role == Some(StepRole::PreparePinnedTools)
                        && matches!(&step.kind, StepKind::Shell { env, .. }
                            if HomeKey::ALL.iter().all(|key| env.contains_key(key.env_name())))
                })
                .map(|at| (id.clone(), at))
        })
        .ok_or_else(|| "no bound job has a toolchain step carrying the homes".into())
}

fn other_shell_step(
    jobs: &BTreeMap<String, Job>,
    id: &str,
    skip: usize,
) -> Result<usize, Box<dyn Error>> {
    jobs[id]
        .steps
        .iter()
        .enumerate()
        .position(|(at, step)| at != skip && matches!(step.kind, StepKind::Shell { .. }))
        .ok_or_else(|| "the bound job has no second shell step".into())
}

fn run_without_homes(
    jobs: &mut BTreeMap<String, Job>,
    id: &str,
    at: usize,
    command: &[&str],
) -> Result<(), Box<dyn Error>> {
    let StepKind::Shell { run, env, .. } =
        &mut jobs.get_mut(id).ok_or("job vanished")?.steps[at].kind
    else {
        return Err("not a shell step".into());
    };
    *run = command.iter().map(|word| (*word).to_owned()).collect();
    for key in HomeKey::ALL {
        env.remove(key.env_name());
    }
    Ok(())
}

fn step_name(jobs: &BTreeMap<String, Job>, id: &str, at: usize) -> String {
    jobs[id].steps[at].name.clone()
}

#[test]
fn w2_names_a_toolchain_step_without_the_homes_and_a_wrong_home() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (_, mut jobs) = finalized(policy)?;
        assert_eq!(waste_findings(&jobs), Vec::new(), "{policy:?}");
        let (id, at) = toolchain_step(&jobs)?;
        let name = step_name(&jobs, &id, at);
        let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
            .ok_or("not a shell step")?;
        env.insert("CARGO_HOME".to_owned(), "/tmp/other".to_owned());
        assert_eq!(
            waste_findings(&jobs),
            vec![WasteFinding::ConflictingHome {
                job: id.clone(),
                step: name.clone(),
                key: HomeKey::CargoHome,
                expected: "${{ runner.temp }}/velnor/cargo".to_owned(),
                found: "/tmp/other".to_owned(),
            }],
            "{policy:?}"
        );
        let env = home_env(&mut jobs.get_mut(&id).ok_or("job vanished")?.steps[at])
            .ok_or("not a shell step")?;
        for key in HomeKey::ALL {
            env.remove(key.env_name());
        }
        assert_eq!(
            waste_findings(&jobs),
            vec![WasteFinding::MissingHome {
                job: id.clone(),
                step: name,
                word: ExecWord::Mise,
                missing: HomeKey::ALL.to_vec(),
            }],
            "{policy:?}"
        );
        let before = waste_findings(&jobs);
        let other = other_shell_step(&jobs, &id, at)?;
        let other_name = step_name(&jobs, &id, other);
        run_without_homes(&mut jobs, &id, other, &["echo", "ready"])?;
        assert_eq!(waste_findings(&jobs), before, "{policy:?}");
        run_without_homes(&mut jobs, &id, other, &["cargo", "build"])?;
        let mut expected = before;
        expected.push(WasteFinding::MissingHome {
            job: id,
            step: other_name,
            word: ExecWord::Cargo,
            missing: HomeKey::ALL.to_vec(),
        });
        expected.sort();
        assert_eq!(waste_findings(&jobs), expected, "{policy:?}");
    }
    Ok(())
}

fn plain_shell_step(jobs: &BTreeMap<String, Job>) -> Result<(String, usize), Box<dyn Error>> {
    jobs.iter()
        .filter(|(_, job)| job.env == JobEnv::Unset)
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .position(|step| matches!(step.kind, StepKind::Shell { .. }))
                .map(|at| (id.clone(), at))
        })
        .ok_or_else(|| "no unbound job has a shell step".into())
}

fn set_auto_install(
    jobs: &mut BTreeMap<String, Job>,
    id: &str,
    at: usize,
    value: &str,
) -> Result<(), Box<dyn Error>> {
    let env = home_env(&mut jobs.get_mut(id).ok_or("job vanished")?.steps[at])
        .ok_or("not a shell step")?;
    env.insert(AUTO_INSTALL_ENV.to_owned(), value.to_owned());
    Ok(())
}

#[test]
fn w3_flags_a_rust_job_without_auto_install_off_and_any_step_override() -> Result<(), Box<dyn Error>>
{
    for policy in POLICIES {
        let (_, mut jobs) = finalized(policy)?;
        let (rust_id, rust_at) = toolchain_step(&jobs)?;
        let (plain_id, plain_at) = plain_shell_step(&jobs)?;
        let plain_name = step_name(&jobs, &plain_id, plain_at);
        set_auto_install(&mut jobs, &plain_id, plain_at, "0")?;
        assert_eq!(waste_findings(&jobs), Vec::new(), "{policy:?}");
        set_auto_install(&mut jobs, &plain_id, plain_at, "1")?;
        assert_eq!(
            waste_findings(&jobs),
            vec![WasteFinding::AutoInstallNotDisabled {
                job: plain_id,
                step: Some(plain_name),
                found: Some("1".to_owned()),
            }],
            "{policy:?}"
        );
        let rust_name = step_name(&jobs, &rust_id, rust_at);
        set_auto_install(&mut jobs, &rust_id, rust_at, "1")?;
        assert!(
            waste_findings(&jobs).contains(&WasteFinding::AutoInstallNotDisabled {
                job: rust_id.clone(),
                step: Some(rust_name),
                found: Some("1".to_owned()),
            }),
            "{policy:?}"
        );
        jobs.get_mut(&rust_id).ok_or("job vanished")?.env = JobEnv::Unset;
        assert!(
            waste_findings(&jobs).contains(&WasteFinding::AutoInstallNotDisabled {
                job: rust_id,
                step: None,
                found: None,
            }),
            "{policy:?}"
        );
    }
    Ok(())
}

#[test]
fn a_step_level_auto_install_override_fails_generation_and_names_the_job()
-> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (_repo, mut prepared) = prepared_for(policy)?;
        finalized_jobs(&prepared)?;
        let mut edited = Vec::new();
        for (id, job) in &mut prepared.workflow.ir.jobs {
            if id == PLAN_JOB_ID || id == REQUIRED_JOB_ID || edited.len() == 2 {
                continue;
            }
            let Some(step) = job.steps.iter_mut().find_map(|step| {
                let name = step.name.clone();
                home_env(step).map(|env| {
                    env.insert(AUTO_INSTALL_ENV.to_owned(), "1".to_owned());
                    name
                })
            }) else {
                continue;
            };
            edited.push((id.clone(), step));
        }
        assert_eq!(edited.len(), 2, "{policy:?}: not enough shell steps");
        let message = finalized_jobs(&prepared)
            .err()
            .ok_or("an auto-install override generated")?
            .to_string();
        assert!(message.contains("W3"), "{policy:?}: {message}");
        assert!(message.contains("fix:"), "{policy:?}: {message}");
        for (id, step) in edited {
            assert!(message.contains(&id), "{policy:?}: {id} not in {message}");
            assert!(
                message.contains(&step),
                "{policy:?}: {step} not in {message}"
            );
        }
    }
    Ok(())
}

fn assert_no_violations(label: &str, jobs: &[JobUse]) {
    assert!(
        jobs.iter().any(JobUse::is_user),
        "{label}: no toolchain job"
    );
    let found = oracle::violations(jobs);
    assert!(found.is_empty(), "{label}: {found:#?}");
}

fn assert_velnor_repository_guards(files: &Files, jobs: &[JobUse]) -> Result<(), Box<dyn Error>> {
    for path in [CI_PATH, FRESHNESS_PATH] {
        assert!(files.contains_key(path), "{path} is not rendered");
    }
    for id in [
        "cargo-machete",
        "cargo-deny",
        "freshness",
        LEDGER_CLI_JOB_ID,
        PLAN_JOB_ID,
    ] {
        oracle::find(jobs, id)?;
    }
    for id in ["freshness", LEDGER_CLI_JOB_ID, PLAN_JOB_ID] {
        assert!(
            oracle::find(jobs, id)?.is_user(),
            "{id} is not a toolchain job"
        );
    }
    assert!(
        jobs.iter()
            .any(|job| job.job.starts_with(CRATE_JOB_ID_PREFIX) && job.is_user()),
        "no crate job uses the toolchain"
    );
    let deny = oracle::find(jobs, "cargo-deny")?;
    assert!(
        !deny.installs_rust && !deny.words.is_empty(),
        "cargo-deny no longer exercises the clean-cwd exemption: {deny:?}"
    );
    Ok(())
}

#[test]
fn every_toolchain_job_in_the_rendered_trees_has_auto_install_off() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let prepared = prepare(repo.path())?;
        let files = files_of(&render_staged_tree(&prepared)?)?;
        let label = format!("{policy:?}");
        let jobs = oracle::scan(&files, |_| true)?;
        if policy == WorkflowPolicy::VelnorRepositoryV1 {
            assert_velnor_repository_guards(&files, &jobs)?;
        }
        assert_no_violations(&label, &jobs);
    }
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let files = files_of(&render_staged_tree(&prepare(repo.path())?)?)?;
    assert_no_violations("suites", &oracle::scan(&files, |_| true)?);
    Ok(())
}

fn unbound_finding(id: &str) -> WasteFinding {
    WasteFinding::AutoInstallNotDisabled {
        job: id.to_owned(),
        step: None,
        found: None,
    }
}

const MACHETE_SPEC: &str =
    "'http:cargo-machete[url=https://example.invalid/m.tgz,checksum=sha256:00]@0.9.2'";

#[test]
fn w3_flags_a_job_by_the_toolchain_command_its_steps_run() -> Result<(), Box<dyn Error>> {
    let (_, mut jobs) = finalized(WorkflowPolicy::VelnorRepositoryV1)?;
    let (id, at) = plain_shell_step(&jobs)?;
    assert!(!waste_findings(&jobs).contains(&unbound_finding(&id)));
    let flagged: [&[&str]; 5] = [
        &[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            MACHETE_SPEC,
            "--",
            "cargo",
            "machete",
            "crates/a",
        ],
        &["rustup", "show"],
        &["rustc", "--version"],
        &["mbx", "build"],
        &["bash", "-c", "cd work && cargo test"],
    ];
    let clean: [&[&str]; 2] = [
        &[
            "mise",
            "--no-config",
            "exec",
            MACHETE_SPEC,
            "--",
            "cargo-machete",
            "crates/a",
        ],
        &["echo", "cargo-deny", "$CARGO_HOME", "Cargo.toml"],
    ];
    for command in flagged {
        run_without_homes(&mut jobs, &id, at, command)?;
        assert!(
            waste_findings(&jobs).contains(&unbound_finding(&id)),
            "{command:?} was not flagged"
        );
    }
    for command in clean {
        run_without_homes(&mut jobs, &id, at, command)?;
        assert!(
            !waste_findings(&jobs).contains(&unbound_finding(&id)),
            "{command:?} was flagged"
        );
    }
    Ok(())
}

#[test]
fn generation_binds_a_job_that_runs_cargo_without_installing_rust() -> Result<(), Box<dyn Error>> {
    let (_repo, mut prepared) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    let unbound = finalized_jobs(&prepared)?;
    let (id, at) = prepared
        .workflow
        .ir
        .jobs
        .iter()
        .filter(|(id, job)| {
            unbound
                .get(*id)
                .is_some_and(|done| done.env == JobEnv::Unset)
                && job.env == JobEnv::Unset
                && *id != REQUIRED_JOB_ID
        })
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .rposition(|step| matches!(step.kind, StepKind::Shell { .. }))
                .map(|at| (id.clone(), at))
        })
        .ok_or("no unbound job has a shell step")?;
    let StepKind::Shell { run, env, .. } = &mut prepared
        .workflow
        .ir
        .jobs
        .get_mut(&id)
        .ok_or("job vanished")?
        .steps[at]
        .kind
    else {
        return Err("not a shell step".into());
    };
    *run = vec!["cargo".to_owned(), "test".to_owned()];
    for key in HomeKey::ALL {
        env.insert(key.env_name().to_owned(), key.standard_value().to_owned());
    }
    let bound = finalized_jobs(&prepared)?;
    assert_eq!(bound[&id].env, JobEnv::RustToolchain);
    let files = files_of(&render_staged_tree(&prepared)?)?;
    let ci = files.get(CI_PATH).ok_or("CI workflow is missing")?;
    let entries = job_env_entries(ci, rendered_job_block(ci, &id)?)?;
    assert!(auto_install_off(&entries), "{id}: {entries:?}");
    Ok(())
}

#[test]
fn committed_generated_workflows_have_auto_install_off_or_are_listed_uncovered()
-> Result<(), Box<dyn Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = files_of_repo(&root)?;
    let scanned = |path: &str| {
        path != CI_PATH
            && !UNCOVERED_WORKFLOWS
                .iter()
                .any(|name| path.ends_with(&format!("/{name}")))
    };
    let jobs = oracle::scan(&files, scanned)?;
    assert!(
        jobs.iter().any(|job| job.file == FRESHNESS_PATH),
        "freshness.yml is not scanned"
    );
    let found = oracle::violations(&jobs);
    assert!(found.is_empty(), "{found:#?}");
    for name in UNCOVERED_WORKFLOWS {
        let path = format!(".github/workflows/{name}");
        let listed = oracle::scan(&files, |candidate| candidate == path)?;
        assert!(
            !oracle::violations(&listed).is_empty(),
            "{path} is covered now; delist it from UNCOVERED_WORKFLOWS"
        );
    }
    Ok(())
}
