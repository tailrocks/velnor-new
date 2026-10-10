use std::collections::BTreeMap;

use crate::verification_jobs::build_task_jobs::{BuildTaskArtifact, BuildTaskTool};
use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, VerificationRunner, VerificationTask, VerificationTaskSource,
};

use super::{
    HOSTED_TASK_NAME_SUFFIX, SCALE_TASK_NAME_SUFFIX, VerificationTaskPolicy,
    build_verification_task_job, validate_verification_jobs,
};
use crate::MiseSetup;
use crate::verification_jobs::workflow_task_jobs::extend_required_needs;

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

fn policy(id: &str, runner: VerificationRunner) -> VerificationTaskPolicy {
    VerificationTaskPolicy {
        task: VerificationTask {
            id: id.to_owned(),
            mise_task: format!("lint-{id}"),
            source: VerificationTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner,
            timeout_minutes: 10,
        },
        runner_label: runner.runs_on().to_owned(),
        scale_set_token: Some("scale-set:velnor+ubuntu-26.04-scale-set".to_owned()),
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            version: "2026.10.4".to_owned(),
            sha256: "a".repeat(64),
        },
        selected_tools: Vec::new(),
        mise_config_sha256: Some("b".repeat(64)),
        mise_lock_sha256: None,
        rust_toolchain_sha256: None,
    }
}

#[test]
fn task_job_is_unconditional_cache_off_and_credential_scrubbed() {
    let linux = policy("construct-assets", VerificationRunner::LinuxX64);
    let job = build_verification_task_job(&linux, CHECKOUT).expect("fixed task job");
    assert_eq!(job.runs_on, "ubuntu-26.04");
    assert!(job.needs.is_empty());
    assert!(job.condition.is_none());
    assert_eq!(job.timeout_minutes.minutes(), 10);
    let permissions = job.permissions.expect("job permissions are explicit");
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.actions, PermissionLevel::None);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert_eq!(job.steps.len(), 3);
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
            assert_eq!(run.len(), 3);
            assert_eq!(run[0], "bash");
            assert_eq!(run[1], "-c");
            let prelude = format!("{} ", crate::toolchain_env::credential_unset_prelude());
            assert!(run[2].starts_with(&prelude), "{}", run[2]);
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
    let velnor_actions_contract::StepKind::Shell { run, .. } = &job.steps[2].kind else {
        panic!("verification task must be a shell step");
    };
    let script = run.last().expect("Bash script argument");
    assert!(script.contains("mise --no-env --no-hooks run --skip-tools lint-construct-assets"));
    assert!(script.contains("MISE_AUTO_INSTALL=false"));
    assert!(script.contains("MISE_TASK_RUN_AUTO_INSTALL=false"));
    assert!(!script.contains("mise --no-env --locked --no-hooks install"));
}

#[test]
fn selected_prebuilt_tools_are_isolated_and_task_tools_are_skipped_at_runtime() {
    let mut task = policy("locked-lint", VerificationRunner::MacosArm64);
    task.mise_config_sha256 = Some("a".repeat(64));
    task.mise_lock_sha256 = Some("b".repeat(64));
    task.selected_tools = vec![BuildTaskTool {
        key: "aqua:vendor/tool".to_owned(),
        version: "1.2.3".to_owned(),
        backend: "aqua:vendor/tool".to_owned(),
        os: vec!["macos".to_owned()],
        config_options: BTreeMap::new(),
        lock_options: BTreeMap::new(),
        artifact: Some(BuildTaskArtifact {
            checksum: format!("sha256:{}", "c".repeat(64)),
            url: "https://github.com/vendor/tool/releases/download/v1.2.3/tool-aarch64.tar.gz"
                .to_owned(),
            url_api: None,
            signer: None,
            provenance: None,
        }),
    }];
    let job = build_verification_task_job(&task, CHECKOUT).expect("locked verification job");
    assert_eq!(job.steps.len(), 4);
    let velnor_actions_contract::StepKind::Shell { run: install, .. } = &job.steps[2].kind else {
        panic!("selected tools use an isolated install step");
    };
    let install_script = install.last().expect("Bash script argument");
    assert!(install_script.contains("mise --no-env --locked --no-hooks install --jobs 2"));
    assert!(
        install_script.contains(
            "https://github.com/vendor/tool/releases/download/v1.2.3/tool-aarch64.tar.gz"
        )
    );
    assert!(!install_script.contains("codebook-lsp"));
    let velnor_actions_contract::StepKind::Shell { run, .. } = &job.steps[3].kind else {
        panic!("task run is a shell step");
    };
    let run_script = run.last().expect("Bash script argument");
    assert!(
        run_script.contains("mise --no-env --locked --no-hooks run --skip-tools lint-locked-lint")
    );
    assert!(run_script.contains("mise --no-env --no-hooks config ls --json"));
    assert!(!run_script.contains("mise install"));
}

#[test]
fn nested_mbx_task_restores_its_declared_working_directory_after_identity_checks() {
    let mut task = policy("native-mbx", VerificationRunner::LinuxX64);
    task.task.mise_task = "desktop-format-check".to_owned();
    task.task.source = VerificationTaskSource {
        mise_config: "native/mise.toml".to_owned(),
        working_directory: "native".to_owned(),
    };
    task.mise_config_sha256 = Some("a".repeat(64));
    task.mise_lock_sha256 = Some("b".repeat(64));
    task.selected_tools = vec![BuildTaskTool {
        key: "mr-boxington".to_owned(),
        version: "1.23.0".to_owned(),
        backend: "packslip:github.com/jdx/mr-boxington".to_owned(),
        os: vec!["linux".to_owned()],
        config_options: BTreeMap::new(),
        lock_options: BTreeMap::new(),
        artifact: Some(BuildTaskArtifact {
            checksum: format!("sha256:{}", "c".repeat(64)),
            url: "https://github.com/jdx/mr-boxington/releases/download/v1.23.0/mbx-x86_64-unknown-linux-gnu.tar.gz".to_owned(),
            url_api: None,
            signer: None,
            provenance: None,
        }),
    }];

    let job = build_verification_task_job(&task, CHECKOUT).expect("nested MBX verification job");
    let velnor_actions_contract::StepKind::Shell { run, .. } = &job.steps[3].kind else {
        panic!("task run must be a shell step");
    };
    let script = run.last().expect("Bash script argument");
    let declared_cwd = script
        .find("cd -P \"$workspace_root/native\"; task_working_directory=\"$PWD\"")
        .expect("task begins in its declared directory");
    let restore_cwd = script
        .find("test \"$mbx_path\" = \"$mbx_parent/$mbx_base\"; cd -P \"$task_working_directory\"")
        .expect("MBX identity check restores the declared directory");
    let task_run = script
        .find("mise --no-env --locked --no-hooks run --skip-tools desktop-format-check")
        .expect("the declared task runs with its tools preinstalled");

    assert!(
        declared_cwd < restore_cwd && restore_cwd < task_run,
        "{script}"
    );
    assert!(script.contains("export MISE_CEILING_PATHS=\"$workspace_root/.\""));
    assert!(script.contains("test -f \"$workspace_root/native/mise.toml\""));
    assert!(script.contains(r#"case "$path" in "$workspace_root/native/mise.toml")"#));
}

#[test]
fn verification_rejects_cargo_sources_and_unbound_artifacts() {
    let mut policy = policy("unsafe-lint", VerificationRunner::LinuxX64);
    policy.mise_config_sha256 = Some("a".repeat(64));
    policy.mise_lock_sha256 = Some("b".repeat(64));
    policy.selected_tools = vec![BuildTaskTool {
        key: "cargo:example-tool".to_owned(),
        version: "1.2.3".to_owned(),
        backend: "cargo:example-tool".to_owned(),
        os: Vec::new(),
        config_options: BTreeMap::new(),
        lock_options: BTreeMap::new(),
        artifact: None,
    }];
    assert!(build_verification_task_job(&policy, CHECKOUT).is_err());

    policy.selected_tools[0].key = "aqua:vendor/tool".to_owned();
    policy.selected_tools[0].backend = "aqua:vendor/tool".to_owned();
    assert!(build_verification_task_job(&policy, CHECKOUT).is_err());
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

#[test]
fn emitted_verification_scripts_are_single_line_without_command_substitution() {
    for runner in [VerificationRunner::LinuxX64, VerificationRunner::MacosArm64] {
        let task = policy("native-format", runner);
        let job = build_verification_task_job(&task, CHECKOUT).expect("task job");
        for step in &job.steps {
            let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
                continue;
            };
            for arg in run {
                assert!(!arg.contains('\n'), "single-line script: {arg}");
                assert!(!arg.contains("$("), "no substitution: {arg}");
                assert!(!arg.contains('`'), "no backticks: {arg}");
            }
        }
    }
}
