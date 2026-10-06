use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, VerificationRunner, VerificationTask, VerificationTaskKind,
};

use super::{
    HOSTED_TASK_NAME_SUFFIX, SCALE_TASK_NAME_SUFFIX, VerificationTaskPolicy,
    build_verification_task_job, extend_required_needs, validate_verification_jobs,
};
use crate::MiseSetup;

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

fn policy(id: &str, runner: VerificationRunner) -> VerificationTaskPolicy {
    VerificationTaskPolicy {
        task: VerificationTask {
            id: id.to_owned(),
            kind: VerificationTaskKind::Verification,
            mise_task: format!("lint-{id}"),
            runner,
            timeout_minutes: 10,
        },
        runner_label: runner.runs_on().to_owned(),
        scale_set_token: Some("scale-set:velnor+ubuntu-26.04-scale-set".to_owned()),
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            version: "2026.9.18".to_owned(),
            sha256: "a".repeat(64),
        },
    }
}

#[test]
fn task_job_is_unconditional_cache_off_and_credential_scrubbed() {
    let linux = policy("construct-assets", VerificationRunner::LinuxX64);
    let job = build_verification_task_job(&linux, CHECKOUT).expect("fixed task job");
    assert_eq!(job.runs_on, "ubuntu-26.04");
    assert_eq!(job.needs, [] as [std::string::String; 0]);
    assert!(job.condition.is_none());
    assert_eq!(job.timeout_minutes.minutes(), 10);
    let permissions = job.permissions.expect("job permissions are explicit");
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.actions, PermissionLevel::None);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert_eq!(job.steps.len(), 4);
    if let velnor_actions_contract::StepKind::Action { uses, with, env } = &job.steps[0].kind {
        assert_eq!(uses.as_str(), CHECKOUT);
        assert_eq!(
            with.get("persist-credentials").map(String::as_str),
            Some("false")
        );
        assert!(env.is_empty());
    } else {
        panic!("checkout must be a pinned action");
    }
    let mise = &job.steps[1];
    if let velnor_actions_contract::StepKind::Action { with, env, .. } = &mise.kind {
        assert!(env.is_empty());
        assert_eq!(with.get("install").map(String::as_str), Some("false"));
        assert_eq!(with.get("env").map(String::as_str), Some("false"));
        assert_eq!(with.get("cache").map(String::as_str), Some("false"));
        assert_eq!(with.get("cache_save").map(String::as_str), Some("false"));
    } else {
        panic!("Mise setup must be an action");
    }
    for step in &job.steps[2..] {
        if let velnor_actions_contract::StepKind::Shell { run, env } = &step.kind {
            let unset = crate::toolchain_env::with_env_unset_argv(&[]);
            assert!(run.starts_with(&unset));
            for variable in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST {
                assert!(
                    env.get(variable).is_some_and(String::is_empty),
                    "{variable}"
                );
            }
        } else {
            panic!("Mise task commands must be shell steps");
        }
    }
    if let velnor_actions_contract::StepKind::Shell { run, .. } = &job.steps[2].kind {
        assert!(run.ends_with(&[
            "mise".to_owned(),
            "install".to_owned(),
            "--locked".to_owned(),
        ]));
    }
    if let velnor_actions_contract::StepKind::Shell { run, .. } = &job.steps[3].kind {
        assert!(run.ends_with(&[
            "mise".to_owned(),
            "run".to_owned(),
            "lint-construct-assets".to_owned(),
        ]));
    }
}

#[test]
fn mixed_linux_and_apple_arm_tasks_keep_distinct_runners() {
    let linux = policy("linux-lint", VerificationRunner::LinuxX64);
    let macos = policy("native-format", VerificationRunner::MacosArm64);
    let linux_job = build_verification_task_job(&linux, CHECKOUT).expect("linux job");
    let macos_job = build_verification_task_job(&macos, CHECKOUT).expect("macos job");
    assert_eq!(linux_job.runs_on, "ubuntu-26.04");
    assert_eq!(macos_job.runs_on, "macos-15");

    let jobs = BTreeMap::from([(linux.job_id(), linux_job), (macos.job_id(), macos_job)]);
    let ids = validate_verification_jobs(&jobs, &[linux, macos], CHECKOUT)
        .expect("both platform jobs satisfy policy");
    assert_eq!(ids, ["task-linux-lint", "task-native-format"]);
}

#[test]
fn all_declared_tasks_join_required_fan_in() {
    let linux = policy("linux-lint", VerificationRunner::LinuxX64);
    let macos = policy("native-format", VerificationRunner::MacosArm64);
    let mut jobs = BTreeMap::from([
        (
            linux.job_id(),
            build_verification_task_job(&linux, CHECKOUT).expect("linux job"),
        ),
        (
            macos.job_id(),
            build_verification_task_job(&macos, CHECKOUT).expect("macOS job"),
        ),
        (
            crate::render::FINAL_JOB_ID.to_owned(),
            Job {
                display_name: "Required".to_owned(),
                runs_on: "ubuntu-26.04".to_owned(),
                check_runner: None,
                timeout_minutes: JobTimeout::PLAN,
                needs: vec!["plan".to_owned()],
                condition: None,
                permissions: None,
                environment: None,
                steps: Vec::new(),
            },
        ),
    ]);
    let ids = validate_verification_jobs(&jobs, &[linux, macos], CHECKOUT)
        .expect("all declared task jobs satisfy policy");

    extend_required_needs(&mut jobs, &ids).expect("required job exists");
    assert_eq!(
        jobs[crate::render::FINAL_JOB_ID].needs,
        vec![
            "plan".to_owned(),
            "task-linux-lint".to_owned(),
            "task-native-format".to_owned(),
        ]
    );
}

#[test]
fn paired_task_lanes_are_complete_and_join_required_together() {
    let linux = policy("linux-lint", VerificationRunner::LinuxX64);
    let base = linux.job_id();
    let hosted_id = format!("{base}__hosted");
    let scale_id = format!("{base}__local");
    let mut hosted = build_verification_task_job(&linux, CHECKOUT).expect("hosted job");
    hosted.display_name.push_str(HOSTED_TASK_NAME_SUFFIX);
    let incomplete = BTreeMap::from([(hosted_id.clone(), hosted.clone())]);
    let error = validate_verification_jobs(&incomplete, std::slice::from_ref(&linux), CHECKOUT)
        .expect_err("a single lane must not qualify as a pair");
    assert!(
        error
            .to_string()
            .contains("verification_task_lane_pair_incomplete")
    );

    let mut scale = build_verification_task_job(&linux, CHECKOUT).expect("scale-set job");
    scale.display_name.push_str(SCALE_TASK_NAME_SUFFIX);
    scale.runs_on = "scale-set:velnor+ubuntu-26.04-scale-set".to_owned();
    let mut jobs = BTreeMap::from([
        (hosted_id.clone(), hosted),
        (scale_id.clone(), scale),
        (
            crate::render::FINAL_JOB_ID.to_owned(),
            Job {
                display_name: "Required".to_owned(),
                runs_on: "ubuntu-26.04".to_owned(),
                check_runner: None,
                timeout_minutes: JobTimeout::PLAN,
                needs: vec!["plan".to_owned()],
                condition: None,
                permissions: None,
                environment: None,
                steps: Vec::new(),
            },
        ),
    ]);
    let ids = validate_verification_jobs(&jobs, &[linux], CHECKOUT)
        .expect("the complete paired task jobs satisfy policy");
    extend_required_needs(&mut jobs, &ids).expect("required job exists");
    assert_eq!(ids, [hosted_id, scale_id]);
    assert_eq!(
        jobs[crate::render::FINAL_JOB_ID].needs,
        vec!["plan", "task-linux-lint__hosted", "task-linux-lint__local"]
    );
}

#[test]
fn task_job_contract_rejects_conditions_dependencies_and_extra_steps() {
    let task = policy("native-format", VerificationRunner::MacosArm64);
    let mut job = build_verification_task_job(&task, CHECKOUT).expect("task job");
    job.condition = Some("always()".to_owned());
    let jobs = BTreeMap::from([(task.job_id(), job)]);
    let error = validate_verification_jobs(&jobs, &[task], CHECKOUT)
        .expect_err("conditional task cannot pass");
    assert!(error.to_string().contains("verification_job_contract"));
}
