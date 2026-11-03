// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_actionlint::actions::ATTEST_USES;
use velnor_actions_contract::workflow::state_seal::{
    STATE_CANARY_PRODUCER_JOB_ID, STATE_CANARY_SEAL_JOB_ID, STATE_CANARY_UNVERIFIED_TEST,
    STATE_SEAL_SUBJECT_EXPR, STATE_SEAL_SUBJECT_REL,
};
use velnor_actions_contract::workflow::{DEPENDENCY_SEED_JOB_CONDITION, ProducerKind};
use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, CacheAccess, CacheWriter, Job, PermissionLevel, Permissions, Step,
    StepKind, StepRole, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

use super::one_job_per_crate::mbx_repo_for;
use super::waste_oracle::files_of;

const CI_PATH: &str = ".github/workflows/ci.yml";

struct Generated {
    ir: WorkflowIr,
    jobs: BTreeMap<String, Job>,
    ci: String,
}

fn generate() -> Result<Generated, Box<dyn Error>> {
    let repo = mbx_repo_for(false, WorkflowPolicy::VelnorRepositoryV1)?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    let tree = render_staged_tree(&prepared)?;
    let files = files_of(&tree)?;
    let ci = files.get(CI_PATH).ok_or("ci.yml was not rendered")?.clone();
    Ok(Generated {
        ir: prepared.workflow.ir.clone(),
        jobs,
        ci,
    })
}

fn job<'a>(jobs: &'a BTreeMap<String, Job>, id: &str) -> Result<&'a Job, Box<dyn Error>> {
    jobs.get(id)
        .ok_or_else(|| format!("{id} was not generated").into())
}

fn texts(job: &Job) -> Vec<&str> {
    let mut texts: Vec<&str> = job.condition.iter().map(String::as_str).collect();
    for step in &job.steps {
        texts.extend(step.condition.iter().map(String::as_str));
        match &step.kind {
            StepKind::Action { with, env, .. } => {
                texts.extend(with.values().map(String::as_str));
                texts.extend(env.values().map(String::as_str));
            }
            StepKind::Shell { run, env } => {
                texts.extend(run.iter().map(String::as_str));
                texts.extend(env.values().map(String::as_str));
            }
            _ => {}
        }
    }
    texts
}

fn needs_reads(job: &Job) -> Vec<String> {
    let mut reads = Vec::new();
    for text in texts(job) {
        let mut rest = text;
        while let Some(at) = rest.find("needs.") {
            rest = &rest[at + "needs.".len()..];
            let end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')))
                .unwrap_or(rest.len());
            reads.push(rest[..end].to_owned());
        }
    }
    reads
}

fn step_of(job: &Job, role: StepRole) -> Result<&Step, Box<dyn Error>> {
    job.steps
        .iter()
        .find(|step| step.role == Some(role))
        .ok_or_else(|| format!("missing {role:?}").into())
}

fn with_of(step: &Step) -> Result<&BTreeMap<String, String>, Box<dyn Error>> {
    match &step.kind {
        StepKind::Action { with, .. } => Ok(with),
        _ => Err("not an action step".into()),
    }
}

fn effective(job: &Job, ir: &WorkflowIr) -> Permissions {
    job.permissions
        .clone()
        .unwrap_or_else(|| ir.permissions.clone())
}

fn rejection(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    why: &str,
) -> Result<String, Box<dyn Error>> {
    let mut candidate = ir.clone();
    candidate.jobs.clone_from(jobs);
    match candidate.validate() {
        Err(error) => Ok(error.to_string()),
        Ok(()) => Err(format!("generation accepted: {why}").into()),
    }
}

fn build_job_has_no_signing_power(generated: &Generated) -> Result<(), Box<dyn Error>> {
    let build = job(&generated.jobs, STATE_CANARY_PRODUCER_JOB_ID)?;
    let gate = format!("{DEPENDENCY_SEED_JOB_CONDITION} && {STATE_CANARY_UNVERIFIED_TEST}");
    assert_eq!(build.producer_kind(), Some(ProducerKind::StateSealBuild));
    assert_eq!(build.cache_access, CacheAccess::Read);
    assert_eq!(build.condition.as_deref(), Some(gate.as_str()));
    let permissions = effective(build, &generated.ir);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert_eq!(permissions.attest, PermissionLevel::None);
    let uploads: Vec<&Step> = build
        .steps
        .iter()
        .filter(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/upload-artifact@")))
        .collect();
    assert_eq!(uploads.len(), 1);
    assert_eq!(
        with_of(uploads[0])?
            .get("retention-days")
            .map(String::as_str),
        Some("1")
    );
    Ok(())
}

fn seal_job_checks_out_nothing_and_signs_one_subject(
    generated: &Generated,
) -> Result<(), Box<dyn Error>> {
    let seal = job(&generated.jobs, STATE_CANARY_SEAL_JOB_ID)?;
    assert_eq!(seal.producer_kind(), Some(ProducerKind::StateSeal));
    assert_eq!(
        seal.cache_access,
        CacheAccess::Writer(CacheWriter::StateSeal)
    );
    assert_eq!(seal.cache_access.mode(), "write-only");
    assert_eq!(
        seal.permissions,
        Some(Permissions {
            contents: PermissionLevel::None,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::Write,
            actions: PermissionLevel::None,
            packages: PermissionLevel::None,
            attest: PermissionLevel::Write,
        })
    );
    assert_eq!(
        seal.needs,
        vec!["plan".to_owned(), STATE_CANARY_PRODUCER_JOB_ID.to_owned()]
    );
    assert!(
        seal.condition
            .as_deref()
            .is_some_and(|condition| condition.starts_with(DEPENDENCY_SEED_JOB_CONDITION))
    );
    let roles: Vec<Option<StepRole>> = seal.steps.iter().map(|step| step.role).collect();
    assert_eq!(
        roles,
        vec![
            Some(StepRole::StateSealDownload),
            Some(StepRole::StateSealSubject),
            Some(StepRole::StateSealAttest),
            Some(StepRole::StateSealSave),
        ]
    );
    for step in &seal.steps {
        assert!(!matches!(
            &step.kind,
            StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. }
        ));
        if let StepKind::Action { uses, .. } = &step.kind {
            assert!(!uses.starts_with("./") && !uses.starts_with("actions/checkout"));
        }
    }
    let attest = step_of(seal, StepRole::StateSealAttest)?;
    assert!(matches!(&attest.kind, StepKind::Action { uses, .. } if uses == ATTEST_USES));
    assert_eq!(
        with_of(attest)?.get("subject-path").map(String::as_str),
        Some(STATE_SEAL_SUBJECT_EXPR)
    );
    let subject = step_of(seal, StepRole::StateSealSubject)?;
    assert!(matches!(&subject.kind, StepKind::Shell { run, .. }
        if run.iter().any(|arg| arg.contains(STATE_SEAL_SUBJECT_REL))));
    for read in needs_reads(seal) {
        let allowed = read.starts_with("plan.outputs.")
            || read == format!("{STATE_CANARY_PRODUCER_JOB_ID}.outputs.artifact_id")
            || read == format!("{STATE_CANARY_PRODUCER_JOB_ID}.result");
        assert!(allowed, "the seal job reads needs.{read}");
    }
    Ok(())
}

fn signing_stays_on_the_seal_job(generated: &Generated) {
    for (id, other) in &generated.jobs {
        let permissions = effective(other, &generated.ir);
        assert_eq!(
            permissions.id_token == PermissionLevel::Write,
            id == STATE_CANARY_SEAL_JOB_ID || other.environment.is_some(),
            "{id}"
        );
        if id != STATE_CANARY_SEAL_JOB_ID {
            assert_eq!(permissions.attest, PermissionLevel::None, "{id}");
        }
    }
    assert_eq!(generated.ci.matches("id-token: write").count(), 1);
    assert_eq!(generated.ci.matches("cache-mode: write-only").count(), 1);
}

fn required_gates_ignore_the_seal_jobs(generated: &Generated) {
    let expected = generated
        .ci
        .lines()
        .filter(|line| line.contains("VELNOR_NEEDS_EXPECTED"))
        .collect::<String>();
    for id in [STATE_CANARY_PRODUCER_JOB_ID, STATE_CANARY_SEAL_JOB_ID] {
        for gate_id in ["required", "required-gate"] {
            if let Some(gate_job) = generated.jobs.get(gate_id) {
                assert!(
                    !gate_job.needs.iter().any(|need| need == id),
                    "{gate_id} needs {id}"
                );
            }
        }
        assert!(!expected.contains(id), "VELNOR_NEEDS_EXPECTED lists {id}");
    }
}

fn exemption_covers_only_the_seal_kind(generated: &Generated) -> Result<(), Box<dyn Error>> {
    let (ir, jobs) = (&generated.ir, &generated.jobs);
    let signing = Permissions {
        id_token: PermissionLevel::Write,
        ..Permissions::default()
    };

    let mut build_signs = jobs.clone();
    build_signs
        .get_mut(STATE_CANARY_PRODUCER_JOB_ID)
        .ok_or("no build job")?
        .permissions = Some(signing.clone());
    rejection(ir, &build_signs, "the build job may sign")?;

    let crate_id = jobs
        .keys()
        .find(|id| id.starts_with(CRATE_JOB_ID_PREFIX))
        .ok_or("no crate job")?;
    let mut crate_signs = jobs.clone();
    crate_signs
        .get_mut(crate_id)
        .ok_or("no crate job")?
        .permissions = Some(signing);
    let error = rejection(ir, &crate_signs, "a crate job may sign")?;
    assert!(
        error.contains("id_token_write_needs_environment"),
        "{error}"
    );

    let plan_checkout = jobs
        .get("plan")
        .and_then(|plan| {
            plan.steps
                .iter()
                .find(|step| step.role == Some(StepRole::Checkout))
        })
        .ok_or("no checkout step")?
        .clone();
    let mut checkout = jobs.clone();
    checkout
        .get_mut(STATE_CANARY_SEAL_JOB_ID)
        .ok_or("no seal job")?
        .steps
        .insert(0, plan_checkout);
    let error = rejection(ir, &checkout, "the seal job may check out")?;
    assert!(error.contains("mixed_or_invalid_producer"), "{error}");

    let mut wrong_role = jobs.clone();
    wrong_role
        .get_mut(STATE_CANARY_SEAL_JOB_ID)
        .ok_or("no seal job")?
        .cache_access = CacheAccess::Writer(CacheWriter::SourcesProducer);
    let error = rejection(ir, &wrong_role, "the seal job may hold another writer role")?;
    assert!(error.contains("cache_writer_not_a_producer"), "{error}");
    Ok(())
}

#[test]
fn seal_jobs_split_signing_from_compiling() -> Result<(), Box<dyn Error>> {
    let generated = generate()?;
    build_job_has_no_signing_power(&generated)?;
    seal_job_checks_out_nothing_and_signs_one_subject(&generated)?;
    signing_stays_on_the_seal_job(&generated);
    required_gates_ignore_the_seal_jobs(&generated);
    assert!(
        generated
            .jobs
            .keys()
            .any(|id| id.starts_with(CRATE_JOB_ID_PREFIX))
    );
    exemption_covers_only_the_seal_kind(&generated)
}
