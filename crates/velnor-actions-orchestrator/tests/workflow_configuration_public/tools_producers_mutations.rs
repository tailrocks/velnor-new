// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::config::{CheckExecutor, CheckPlatform, CheckRunner};
use velnor_actions_contract::workflow::{RUST_VERIFIER_USES, SUITE_PRODUCER_JOB_CONDITION};
use velnor_actions_contract::{
    Job, PermissionLevel, Permissions, ServiceContainer, Step, StepKind, StepRole, WorkflowPolicy,
};

use super::tools_producers::{finalized, is_tools_producer, is_upload, positions};

const INVALID_PRODUCER: &str = "mixed_or_invalid_producer";
const CHECK_JOB_ONLY: &str = "check_runner_requires_check_job";
const FOREIGN_PREFIX_ID: &str = "tools-producer-zzzzzzzz";
const FOREIGN_ID: &str = "crate-x";

type Jobs = BTreeMap<String, Job>;
type Apply = fn(&mut Jobs, &str, &Donors) -> Result<(), Box<dyn Error>>;

struct Mutation {
    name: &'static str,
    rejected_as: &'static str,
    apply: Apply,
}

struct Donors {
    task: Step,
    upload: Step,
    check: Step,
    check_runner: CheckRunner,
    crate_job: Job,
    permissions: Permissions,
    id_token: Permissions,
}

fn mutation(name: &'static str, rejected_as: &'static str, apply: Apply) -> Mutation {
    Mutation {
        name,
        rejected_as,
        apply,
    }
}

fn producer<'a>(jobs: &'a mut Jobs, id: &str) -> Result<&'a mut Job, Box<dyn Error>> {
    jobs.get_mut(id).ok_or_else(|| "producer vanished".into())
}

fn step_at(job: &Job, role: StepRole) -> Result<usize, Box<dyn Error>> {
    positions(job, role)
        .first()
        .copied()
        .ok_or_else(|| format!("producer has no {role:?} step").into())
}

fn clone_of(job: &Job, role: StepRole) -> Result<Step, Box<dyn Error>> {
    Ok(job.steps[step_at(job, role)?].clone())
}

fn rewrite_probe(
    jobs: &mut Jobs,
    id: &str,
    edit: impl FnOnce(&mut BTreeMap<String, String>, &mut String),
) -> Result<(), Box<dyn Error>> {
    let job = producer(jobs, id)?;
    let at = step_at(job, StepRole::ToolsCacheRestore)?;
    let StepKind::Action { uses, with, .. } = &mut job.steps[at].kind else {
        return Err("probe is not an action".into());
    };
    edit(with, uses);
    Ok(())
}

fn mutations() -> Vec<Mutation> {
    let mut all = job_shape_mutations();
    all.extend(step_shape_mutations());
    all.extend(identity_mutations());
    all
}

fn job_shape_mutations() -> Vec<Mutation> {
    vec![
        mutation("needs_plan", INVALID_PRODUCER, |jobs, id, _| {
            producer(jobs, id)?.needs.push("plan".to_owned());
            Ok(())
        }),
        mutation(
            "task_execution_step",
            INVALID_PRODUCER,
            |jobs, id, donors| {
                producer(jobs, id)?.steps.push(donors.task.clone());
                Ok(())
            },
        ),
        mutation("upload_step", INVALID_PRODUCER, |jobs, id, donors| {
            producer(jobs, id)?.steps.push(donors.upload.clone());
            Ok(())
        }),
        mutation(
            "check_job_role_step",
            INVALID_PRODUCER,
            |jobs, id, donors| {
                producer(jobs, id)?.steps.push(donors.check.clone());
                Ok(())
            },
        ),
        mutation("contents_write", INVALID_PRODUCER, |jobs, id, donors| {
            producer(jobs, id)?.permissions = Some(donors.permissions.clone());
            Ok(())
        }),
        mutation("id_token_write", INVALID_PRODUCER, |jobs, id, donors| {
            producer(jobs, id)?.permissions = Some(donors.id_token.clone());
            Ok(())
        }),
        mutation("environment", INVALID_PRODUCER, |jobs, id, _| {
            producer(jobs, id)?.environment = Some("production".to_owned());
            Ok(())
        }),
        mutation("check_runner", CHECK_JOB_ONLY, |jobs, id, donors| {
            producer(jobs, id)?.check_runner = Some(donors.check_runner.clone());
            Ok(())
        }),
        mutation("services", INVALID_PRODUCER, |jobs, id, _| {
            producer(jobs, id)?
                .services
                .push(ServiceContainer::postgres(18)?);
            Ok(())
        }),
        mutation("outputs", INVALID_PRODUCER, |jobs, id, _| {
            producer(jobs, id)?.outputs.insert(
                "leak".to_owned(),
                "${{ steps.v2.outputs.identity }}".to_owned(),
            );
            Ok(())
        }),
        mutation(
            "scheduled_condition_in_ci_workflow",
            INVALID_PRODUCER,
            |jobs, id, _| {
                producer(jobs, id)?.condition = Some(SUITE_PRODUCER_JOB_CONDITION.to_owned());
                Ok(())
            },
        ),
        mutation("dropped_ref_protected", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            job.condition = job
                .condition
                .as_ref()
                .map(|condition| condition.replace(" && github.ref_protected == true", ""));
            Ok(())
        }),
    ]
}

fn step_shape_mutations() -> Vec<Mutation> {
    let mut all = ordering_mutations();
    all.extend(role_count_mutations());
    all
}

fn role_count_mutations() -> Vec<Mutation> {
    vec![
        mutation("parallel_step", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let at = step_at(job, StepRole::PreparePinnedTools)?;
            job.steps[at].parallel = true;
            Ok(())
        }),
        mutation("duplicate_checkout", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let copy = clone_of(job, StepRole::Checkout)?;
            job.steps.insert(1, copy);
            Ok(())
        }),
        mutation("duplicate_save", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let copy = clone_of(job, StepRole::ToolsCacheSave)?;
            job.steps.push(copy);
            Ok(())
        }),
        mutation("duplicate_probe", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let at = step_at(job, StepRole::ToolsCacheRestore)?;
            let mut copy = job.steps[at].clone();
            copy.id = None;
            job.steps.insert(at + 1, copy);
            Ok(())
        }),
        mutation("missing_scrub", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let at = step_at(job, StepRole::ToolsCacheScrub)?;
            job.steps.remove(at);
            Ok(())
        }),
    ]
}

fn ordering_mutations() -> Vec<Mutation> {
    vec![
        mutation("scrub_before_probe", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let probe = step_at(job, StepRole::ToolsCacheRestore)?;
            let scrub = step_at(job, StepRole::ToolsCacheScrub)?;
            job.steps.swap(probe, scrub);
            Ok(())
        }),
        mutation("save_before_installs", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let save = job.steps.remove(step_at(job, StepRole::ToolsCacheSave)?);
            let first_install = step_at(job, StepRole::PreparePinnedTools)?;
            job.steps.insert(first_install, save);
            Ok(())
        }),
        mutation("install_after_save", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let copy = clone_of(job, StepRole::PreparePinnedTools)?;
            job.steps.push(copy);
            Ok(())
        }),
        mutation("ungated_install", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let at = step_at(job, StepRole::PreparePinnedTools)?;
            job.steps[at].condition = None;
            Ok(())
        }),
        mutation("without_install", INVALID_PRODUCER, |jobs, id, _| {
            producer(jobs, id)?
                .steps
                .retain(|step| step.role != Some(StepRole::PreparePinnedTools));
            Ok(())
        }),
        mutation("restored_verifier", INVALID_PRODUCER, |jobs, id, _| {
            let job = producer(jobs, id)?;
            let mut verifier = clone_of(job, StepRole::PreparePinnedTools)?;
            verifier.role = Some(StepRole::VerifyRustManifest);
            verifier.kind = StepKind::Action {
                uses: RUST_VERIFIER_USES[0].to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            };
            let at = step_at(job, StepRole::ToolsCacheSave)?;
            job.steps.insert(at, verifier);
            Ok(())
        }),
    ]
}

fn identity_mutations() -> Vec<Mutation> {
    vec![
        mutation(
            "probe_without_lookup_only",
            INVALID_PRODUCER,
            |jobs, id, _| {
                rewrite_probe(jobs, id, |with, _| {
                    with.remove("lookup-only");
                })
            },
        ),
        mutation("probe_unpinned_action", INVALID_PRODUCER, |jobs, id, _| {
            rewrite_probe(jobs, id, |_, uses| {
                *uses = format!("actions/cache/restore@{}", "a".repeat(40));
            })
        }),
        mutation("probe_other_key", INVALID_PRODUCER, |jobs, id, _| {
            rewrite_probe(jobs, id, |with, _| {
                with.insert("key".to_owned(), "mise-tools-v2-other".to_owned());
            })
        }),
        mutation("probe_other_path", INVALID_PRODUCER, |jobs, id, _| {
            rewrite_probe(jobs, id, |with, _| {
                with.insert("path".to_owned(), "~/.cargo".to_owned());
            })
        }),
        mutation(
            "producer_id_without_prefix",
            INVALID_PRODUCER,
            |jobs, id, _| {
                let job = jobs.remove(id).ok_or("producer vanished")?;
                jobs.insert(FOREIGN_ID.to_owned(), job);
                Ok(())
            },
        ),
        mutation(
            "prefixed_id_without_producer",
            INVALID_PRODUCER,
            |jobs, _, donors| {
                jobs.insert(FOREIGN_PREFIX_ID.to_owned(), donors.crate_job.clone());
                Ok(())
            },
        ),
    ]
}

fn donors(jobs: &Jobs) -> Result<Donors, Box<dyn Error>> {
    let find = |wanted: &dyn Fn(&Step) -> bool| {
        jobs.values()
            .flat_map(|job| &job.steps)
            .find(|step| wanted(step))
            .cloned()
    };
    Ok(Donors {
        task: find(&|step| matches!(step.kind, StepKind::TaskExecution { .. }))
            .ok_or("no task step")?,
        upload: find(&is_upload).ok_or("no upload step")?,
        check: find(&|step| step.role == Some(StepRole::CargoDeny)).ok_or("no check step")?,
        check_runner: CheckRunner {
            label: "ubuntu-26.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        crate_job: jobs
            .values()
            .find(|job| {
                job.producer_kind().is_none()
                    && job
                        .steps
                        .iter()
                        .any(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
            })
            .cloned()
            .ok_or("no crate job")?,
        permissions: Permissions {
            contents: PermissionLevel::Write,
            ..Permissions::default()
        },
        id_token: Permissions {
            id_token: PermissionLevel::Write,
            ..Permissions::default()
        },
    })
}

#[test]
fn producer_validation_rejects_each_mutation() -> Result<(), Box<dyn Error>> {
    let (ir, jobs) = finalized(WorkflowPolicy::VelnorRepositoryV1)?;
    let donors = donors(&jobs)?;
    let producer_id = jobs
        .iter()
        .find(|(_, job)| is_tools_producer(job))
        .map(|(id, _)| id.clone())
        .ok_or("no producer")?;
    let validate = |jobs: &Jobs| {
        let mut candidate = ir.clone();
        candidate.jobs.clone_from(jobs);
        candidate.validate()
    };
    validate(&jobs).map_err(|error| format!("unmutated producer rejected: {error}"))?;
    let mut accepted = Vec::new();
    let mut misattributed = Vec::new();
    for Mutation {
        name,
        rejected_as,
        apply,
    } in mutations()
    {
        let mut mutated = jobs.clone();
        apply(&mut mutated, &producer_id, &donors)?;
        match validate(&mutated) {
            Ok(()) => accepted.push(name),
            Err(error) if error.to_string().contains(rejected_as) => {}
            Err(error) => misattributed.push(format!("{name}: {error}")),
        }
    }
    assert_eq!(
        accepted,
        Vec::<&str>::new(),
        "mutations accepted by the producer validator"
    );
    assert_eq!(
        misattributed,
        Vec::<String>::new(),
        "mutations rejected for another reason"
    );
    Ok(())
}
