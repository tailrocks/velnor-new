use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::{
    Job, JobTimeout, MiseTaskSource, ScaleSetSelector, Step, StepKind, StepRole,
    VerificationRunner, VerificationTask,
};

use crate::yaml::Yaml;
use crate::{
    MiseSetup,
    verification_jobs::{VerificationTaskPolicy, WorkflowTaskPolicy},
};

use super::{ACTION_NAME_PREFIX, factor_obligation_steps};

const CHECKOUT: &str = "actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const VERSION: &str = "0.1.6";
const HOSTILE_TASK_ARGUMENT: &str = "crate-0\"; printf injected; #";

#[test]
fn one_typed_action_serves_150_validated_tasks_without_dropping_job_contracts() {
    let jobs = (0..150)
        .map(|index| {
            let id = format!("rust-crate-{index}");
            let job = Job {
                display_name: format!("Rust / crate {index}"),
                runs_on: "ubuntu-26.04".to_owned(),
                check_runner: None,
                timeout_minutes: JobTimeout::new(20).expect("valid timeout"),
                needs: vec!["plan".to_owned()],
                condition: Some("needs.plan.result == 'success'".to_owned()),
                permissions: None,
                environment: None,
                steps: vec![checkout_step(), acquire_step(), task_step(index)],
            };
            (id, job)
        })
        .collect::<BTreeMap<_, _>>();

    let (factored, files) = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect("factor obligations");

    assert_eq!(factored.len(), 150);
    assert_eq!(
        files.len(),
        2,
        "one shared action and one execution manifest"
    );
    let action_file = files
        .iter()
        .find(|file| file.path.ends_with("/action.yml"))
        .expect("shared composite action");
    let manifest_file = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH)
        .expect("versioned task execution manifest");
    assert_eq!(
        action_file.path,
        format!(".github/actions/{ACTION_NAME_PREFIX}0/action.yml")
    );
    assert!(action_file.bytes.contains("using: composite"));
    assert!(action_file.bytes.contains("shell: bash"));
    assert!(manifest_file.bytes.starts_with(&format!(
        "{}\n",
        crate::marker::marker_for_version(VERSION).expect("generator marker")
    )));
    assert!(
        manifest_file
            .bytes
            .contains("\"task_id\":\"stack/rust/crate-0/test/default\"")
    );
    assert!(
        manifest_file
            .bytes
            .contains("crate-0\\\"; printf injected; #")
    );
    assert_eq!(manifest_file.bytes.matches("\"task_id\":").count(), 150);
    assert!(!action_file.bytes.contains("inputs.task_id"));
    assert!(!action_file.bytes.contains("inputs.execution_digest"));
    assert!(action_file.bytes.contains("inputs.digest"));
    let action = task_document_for_test();
    let run = action_run_scalar(&action);
    assert!(run.contains("argv+=(\"$value\")"));
    assert!(run.contains("env -- \"${task_env[@]}\" \"${argv[@]}\""));
    assert!(run.contains("write-task-report-v1"));
    assert!(run.contains("if [ \"$task_code\" -ne 0 ]; then exit \"$task_code\"; fi"));
    assert!(run.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(!run.contains("eval"));
    assert!(!run.contains("replace_runner_temp"));
    assert!(run.contains("[[ \"$value\" != *'${{'* ]] || fail_frame"));
    assert!(run.contains("VELNOR_RUNTIME_RUNNER_TEMP"));
    for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
    {
        assert!(!action_file.bytes.contains(&format!("inputs.env_{key}")));
    }

    for (id, original) in &jobs {
        let rewritten = factored.get(id).expect("job retained");
        assert_eq!(rewritten.display_name, original.display_name);
        assert_eq!(rewritten.runs_on, original.runs_on);
        assert_eq!(rewritten.timeout_minutes, original.timeout_minutes);
        assert_eq!(rewritten.needs, original.needs);
        assert_eq!(rewritten.condition, original.condition);
        assert_eq!(rewritten.permissions, original.permissions);
        assert_eq!(rewritten.environment, original.environment);
        assert_eq!(rewritten.steps.len(), original.steps.len());
        assert_eq!(rewritten.steps[0], original.steps[0]);
        assert_eq!(rewritten.steps[1], original.steps[1]);
        let caller = &rewritten.steps[2];
        assert_eq!(caller.name, original.steps[2].name);
        assert_eq!(caller.condition, original.steps[2].condition);
        let StepKind::Action { uses, with, env } = &caller.kind else {
            panic!("obligation was not replaced by its composite call");
        };
        assert_eq!(uses, "./.github/actions/declared-task-0");
        assert!(
            env.is_empty(),
            "task values cross only through the manifest"
        );
        assert_eq!(
            with.len(),
            1,
            "the full digest selects the validated record"
        );
        assert_eq!(with.keys().next().map(String::as_str), Some("digest"));
        assert_eq!(with["digest"].len(), 67);
        assert!(
            with["digest"].starts_with("b3-")
                && with["digest"][3..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        );
        assert!(!action_file.bytes.contains(HOSTILE_TASK_ARGUMENT));
        assert!(!action_file.bytes.contains("inputs.argv_"));
        assert!(!action_file.bytes.contains("inputs.env_"));
    }
}

#[test]
fn ordinary_shell_and_tofu_steps_stay_unfactored() {
    let shell = crate::steps::shell_step(
        "Tofu remains an ordinary shell step",
        vec!["tofu".to_owned(), "validate".to_owned()],
        BTreeMap::new(),
    )
    .expect("fixed shell step");
    let mut tofu = shell.clone();
    tofu.role = Some(StepRole::TofuProviderUse);
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), shell, tofu]),
    )]);

    let (factored, files) = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect("leave shell tasks alone");
    assert!(files.is_empty());
    assert_eq!(
        factored.keys().collect::<Vec<_>>(),
        jobs.keys().collect::<Vec<_>>()
    );
    for (job_id, original) in &jobs {
        let preserved = &factored[job_id];
        assert_eq!(preserved.display_name, original.display_name);
        assert_eq!(preserved.runs_on, original.runs_on);
        assert_eq!(preserved.check_runner, original.check_runner);
        assert_eq!(preserved.timeout_minutes, original.timeout_minutes);
        assert_eq!(preserved.needs, original.needs);
        assert_eq!(preserved.condition, original.condition);
        assert_eq!(preserved.permissions, original.permissions);
        assert_eq!(preserved.environment, original.environment);
        assert_eq!(preserved.steps, original.steps);
    }
}

#[test]
fn typed_task_requires_checkout_staging_and_credential_free_inputs() {
    let task = task_step(0);
    let no_checkout = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![acquire_step(), task.clone()]),
    )]);
    assert!(
        factor_obligation_steps(&no_checkout, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("checkout is mandatory")
            .to_string()
            .contains("declared_task_requires_checkout")
    );

    let no_stage = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), task.clone()]),
    )]);
    assert!(
        factor_obligation_steps(&no_stage, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("the staged helper is mandatory")
            .to_string()
            .contains("declared_task_requires_staged_helper")
    );

    let mut secret_expression = task.clone();
    let StepKind::TaskExecution { env, .. } = &mut secret_expression.kind else {
        unreachable!();
    };
    env.insert(
        "GITHUB_TOKEN".to_owned(),
        "${{ secrets.GITHUB_TOKEN }}".to_owned(),
    );
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), secret_expression]),
    )]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("only the exact runner.temp expression is supported")
            .to_string()
            .contains("unsupported_declared_task_expression")
    );

    let mut credentialed = task;
    let StepKind::TaskExecution { env, .. } = &mut credentialed.kind else {
        unreachable!();
    };
    env.insert("GITHUB_TOKEN".to_owned(), "fixture-token-value".to_owned());
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), credentialed]),
    )]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("credentials cannot enter task action inputs")
            .to_string()
            .contains("credential_step_env:GITHUB_TOKEN")
    );
}

#[test]
fn typed_task_preserves_only_the_exact_runner_temp_expression() {
    let mut task = task_step(0);
    let StepKind::TaskExecution { env, .. } = &mut task.kind else {
        unreachable!();
    };
    assert_eq!(env["MISE_CARGO_HOME"], "${{ runner.temp }}/velnor/cargo");

    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), task.clone()]),
    )]);
    let (factored, files) = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect("known expression is retained in the manifest");
    let manifest = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH)
        .expect("manifest emitted");
    assert!(manifest.bytes.contains("${{ runner.temp }}/velnor/cargo"));
    let StepKind::Action { with, .. } = &factored["rust-demo"].steps[2].kind else {
        panic!("factored task uses the shared action");
    };
    assert_eq!(with.keys().next().map(String::as_str), Some("digest"));
    assert_eq!(with["digest"].len(), 67);

    let StepKind::TaskExecution { env, .. } = &mut task.kind else {
        unreachable!();
    };
    env.insert(
        "MISE_CARGO_HOME".to_owned(),
        "${{ github.workspace }}/.cargo".to_owned(),
    );
    let unsupported = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), task]),
    )]);
    assert!(
        factor_obligation_steps(&unsupported, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("unmodeled runtime expressions cannot be serialized")
            .to_string()
            .contains("unsupported_declared_task_expression")
    );
}

#[cfg(unix)]
#[test]
fn wrapper_rejects_unresolved_expressions_and_passes_shell_metacharacters_as_data() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let test_root = std::env::temp_dir().join(format!(
        "velnor-task-wrapper-{}-{timestamp}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    let runner_temp = test_root.join("runner temp");
    let helper_dir = runner_temp.join("velnor/bin");
    std::fs::create_dir_all(&helper_dir).expect("create fake runner temp");

    let helper = helper_dir.join(format!("velnor-actions-{VERSION}"));
    let task_runner = test_root.join("task-runner");
    let frame_path = test_root.join("frame.bin");
    let call_log = test_root.join("helper-calls.log");
    let injection_marker = test_root.join("shell-injection-ran");
    let task_started_marker = test_root.join("task-started");
    let payload = format!("$(touch {}; printf injected)", injection_marker.display());
    let expected_execution_digest = format!("b3-{}", "a".repeat(64));
    let plan_digest = format!("b3-{}", "b".repeat(64));
    std::fs::write(
        &task_runner,
        b"#!/bin/sh\nprintf '%s' \"$1\"\n: > \"$TASK_STARTED_MARKER\"\nexit \"$TASK_EXIT_CODE\"\n",
    )
    .expect("write fixed argv probe");
    let mut task_permissions = std::fs::metadata(&task_runner)
        .expect("inspect fixed argv probe")
        .permissions();
    task_permissions.set_mode(0o700);
    std::fs::set_permissions(&task_runner, task_permissions)
        .expect("make fixed argv probe executable");
    let fields = vec![
        "VELNOR-TASK-EXECUTION-V1".to_owned(),
        "stack/rust/crate-0/test/default".to_owned(),
        expected_execution_digest.clone(),
        plan_digest,
        "stack:rust|task:stack/rust/crate-0/test/default".to_owned(),
        "m-0123456789abcdef".to_owned(),
        VERSION.to_owned(),
        "0".to_owned(),
        String::new(),
        "2".to_owned(),
        task_runner.display().to_string(),
        payload,
        "2".to_owned(),
        "TASK_STARTED_MARKER".to_owned(),
        task_started_marker.display().to_string(),
        "TASK_EXIT_CODE".to_owned(),
        "0".to_owned(),
        "END".to_owned(),
    ];
    let encode_frame = |fields: &[String]| {
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(field.as_bytes());
            bytes.push(0);
        }
        bytes
    };
    let valid_frame = encode_frame(&fields);
    std::fs::write(&frame_path, &valid_frame).expect("write valid frame");
    std::fs::write(
        &helper,
        b"#!/bin/sh\nprintf '%s\\n' \"$VELNOR_INTERNAL_OP\" >> \"$VELNOR_TEST_CALL_LOG\"\ncase \"$VELNOR_INTERNAL_OP\" in\n  resolve-task-execution-v1) cat \"$VELNOR_TEST_FRAME\" ;;\n  write-task-report-v1) exit \"${VELNOR_TEST_REPORT_CODE:-0}\" ;;\n  *) exit 9 ;;\nesac\n",
    )
    .expect("write fake helper");
    let mut permissions = std::fs::metadata(&helper)
        .expect("inspect fake helper")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&helper, permissions).expect("make helper executable");

    let script = super::task_script(VERSION);
    let script_path = test_root.join("task-wrapper.sh");
    std::fs::write(&script_path, &script).expect("write generated wrapper for syntax check");
    let syntax = std::process::Command::new("bash")
        .arg("-n")
        .arg(&script_path)
        .output()
        .expect("check generated Bash syntax");
    assert!(
        syntax.status.success(),
        "generated wrapper is not valid Bash: {}",
        String::from_utf8_lossy(&syntax.stderr)
    );
    let run_script = |report_code: &str| {
        std::process::Command::new("bash")
            .arg("-x")
            .arg("-c")
            .arg(&script)
            .env("RUNNER_TEMP", &runner_temp)
            .env(super::RUNTIME_RUNNER_TEMP_ENV, &runner_temp)
            .env(super::GENERATOR_VERSION_ENV, VERSION)
            .env(super::TASK_EXECUTION_DIGEST_ENV, &expected_execution_digest)
            .env("VELNOR_TASK_ID", "stack/rust/crate-0/test/default")
            .env("VELNOR_TEST_FRAME", &frame_path)
            .env("VELNOR_TEST_CALL_LOG", &call_log)
            .env("VELNOR_TEST_REPORT_CODE", report_code)
            .output()
            .expect("run generated Bash composite script")
    };
    let result = run_script("0");
    assert!(
        result.status.success(),
        "script failed: stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        result.stdout.as_slice(),
        fields[11].as_bytes(),
        "the fixed probe receives the hostile literal as one argv value; stderr={}; helper_calls={}; task_started={}",
        String::from_utf8_lossy(&result.stderr),
        std::fs::read_to_string(&call_log).unwrap_or_default(),
        task_started_marker.exists(),
    );
    assert!(
        !injection_marker.exists(),
        "payload is data, never shell source"
    );
    assert!(
        task_started_marker.exists(),
        "valid task reached the fixed probe"
    );
    std::fs::remove_file(&task_started_marker).expect("clear valid task marker");

    let mut task_and_report_fail = fields.clone();
    task_and_report_fail[16] = "7".to_owned();
    std::fs::write(&frame_path, encode_frame(&task_and_report_fail))
        .expect("write task failure frame");
    let task_failure = run_script("9");
    assert_eq!(task_failure.status.code(), Some(7));
    assert!(
        task_started_marker.exists(),
        "task failure reached the probe"
    );
    assert!(
        !injection_marker.exists(),
        "failure payload remained literal"
    );
    std::fs::remove_file(&task_started_marker).expect("clear task failure marker");

    std::fs::write(&frame_path, &valid_frame).expect("restore valid frame");
    let report_failure = run_script("9");
    assert_eq!(report_failure.status.code(), Some(9));
    assert!(
        task_started_marker.exists(),
        "report ran after successful task"
    );
    std::fs::remove_file(&task_started_marker).expect("clear report failure marker");

    let mut bad_count = fields.clone();
    bad_count[9] = "513".to_owned();
    let mut bad_sentinel = fields.clone();
    bad_sentinel[17] = "NOT-END".to_owned();
    let mut malformed_cap_flag = fields.clone();
    malformed_cap_flag[7] = "2".to_owned();
    let mut malformed_cap_value = fields.clone();
    malformed_cap_value[7] = "1".to_owned();
    malformed_cap_value[8] = "0".to_owned();
    let mut wrong_digest = fields.clone();
    wrong_digest[2] = format!("b3-{}", "c".repeat(64));
    let mut malformed_execution_digest = fields.clone();
    malformed_execution_digest[2] = "not-a-digest".to_owned();
    let mut malformed_task_digest = fields.clone();
    malformed_task_digest[3] = "not-a-digest".to_owned();
    let mut malformed_matrix_id = fields.clone();
    malformed_matrix_id[4] = "matrix/test".to_owned();
    let mut malformed_matrix_key = fields.clone();
    malformed_matrix_key[5] = "not-a-key".to_owned();
    let mut duplicate_env_key = fields.clone();
    duplicate_env_key[12] = "3".to_owned();
    duplicate_env_key.splice(
        17..17,
        [
            "TASK_STARTED_MARKER".to_owned(),
            task_started_marker.display().to_string(),
        ],
    );
    let mut unsupported_expression = fields.clone();
    unsupported_expression[14] = "${{ github.workspace }}/marker".to_owned();
    let mut unresolved_runner_temp = fields.clone();
    unresolved_runner_temp[11] = "${{ runner.temp }}/payload".to_owned();
    let mut unresolved_argv = unresolved_runner_temp.clone();
    unresolved_argv[10] = "${{ runner.temp }}/task-runner".to_owned();
    let mut trailing_field = valid_frame.clone();
    trailing_field.extend_from_slice(b"EXTRA\0");
    let mut missing_terminator = valid_frame;
    missing_terminator.pop();
    let invalid_frames = [
        ("bad argv count", encode_frame(&bad_count)),
        ("bad END sentinel", encode_frame(&bad_sentinel)),
        ("malformed cap flag", encode_frame(&malformed_cap_flag)),
        ("malformed cap value", encode_frame(&malformed_cap_value)),
        ("wrong execution digest", encode_frame(&wrong_digest)),
        (
            "malformed execution digest",
            encode_frame(&malformed_execution_digest),
        ),
        (
            "malformed task digest",
            encode_frame(&malformed_task_digest),
        ),
        ("malformed matrix ID", encode_frame(&malformed_matrix_id)),
        ("malformed matrix key", encode_frame(&malformed_matrix_key)),
        ("duplicate env key", encode_frame(&duplicate_env_key)),
        (
            "unsupported expression",
            encode_frame(&unsupported_expression),
        ),
        (
            "unresolved runner.temp value",
            encode_frame(&unresolved_runner_temp),
        ),
        ("unresolved argv", encode_frame(&unresolved_argv)),
        ("trailing field", trailing_field),
        ("missing terminator", missing_terminator),
    ];
    let invalid_frame_count = invalid_frames.len();
    for (case, invalid_frame) in invalid_frames {
        if task_started_marker.exists() {
            std::fs::remove_file(&task_started_marker)
                .expect("clear task marker before rejection case");
        }
        if injection_marker.exists() {
            std::fs::remove_file(&injection_marker)
                .expect("clear injection marker before rejection case");
        }
        std::fs::write(&frame_path, invalid_frame).expect("write invalid frame");
        let rejected = run_script("0");
        assert_eq!(
            rejected.status.code(),
            Some(125),
            "{case}: malformed frame was not rejected: stdout={} stderr={}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(
            !task_started_marker.exists(),
            "{case}: invalid frame reached task execution"
        );
        assert!(
            !injection_marker.exists(),
            "{case}: invalid frame evaluated hostile argv data"
        );
    }
    let expected_calls = format!(
        "resolve-task-execution-v1\nwrite-task-report-v1\nresolve-task-execution-v1\nwrite-task-report-v1\nresolve-task-execution-v1\nwrite-task-report-v1\n{}",
        "resolve-task-execution-v1\n".repeat(invalid_frame_count)
    );
    assert_eq!(
        std::fs::read_to_string(&call_log).expect("record helper operations"),
        expected_calls
    );
    std::fs::remove_dir_all(test_root).expect("remove fake runner tree");
}

#[test]
fn typed_task_rejects_helper_version_from_a_different_generation_context() {
    let mut task = task_step(0);
    let StepKind::TaskExecution {
        report_helper_version,
        ..
    } = &mut task.kind
    else {
        unreachable!();
    };
    *report_helper_version = "0.1.4".to_owned();
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), task]),
    )]);

    let error = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect_err("task metadata cannot name a different helper release than the renderer");
    assert!(
        error
            .to_string()
            .contains("declared_task_helper_version_mismatch:rust-demo"),
        "unexpected error: {error}"
    );
}

#[test]
fn generated_task_action_marker_uses_generator_version_and_body_uses_helper_version() {
    const CONSUMER_HELPER_VERSION: &str = "0.1.4";
    let mut task = task_step(0);
    let StepKind::TaskExecution {
        report_helper_version,
        ..
    } = &mut task.kind
    else {
        unreachable!();
    };
    *report_helper_version = CONSUMER_HELPER_VERSION.to_owned();
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![
            checkout_step(),
            acquire_step_for(CONSUMER_HELPER_VERSION),
            task,
        ]),
    )]);

    let (_, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, CONSUMER_HELPER_VERSION, &[], None)
            .expect("factor task under independently versioned generator/helper");
    assert_eq!(files.len(), 2);
    let action = files
        .iter()
        .find(|file| file.path.ends_with("/action.yml"))
        .expect("action output");
    let manifest = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH)
        .expect("manifest output");
    assert!(action.bytes.starts_with(&format!(
        "{}\n",
        crate::marker::marker_for_version(VERSION).expect("generator marker")
    )));
    assert!(
        action
            .bytes
            .contains("$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.4")
    );
    assert!(manifest.bytes.starts_with(&format!(
        "{}\n",
        crate::marker::marker_for_version(VERSION).expect("generator marker")
    )));
    assert!(
        manifest
            .bytes
            .contains("\"report_helper_version\":\"0.1.4\"")
    );
}

#[test]
fn scale_set_task_requires_a_resolved_linux_x64_verification_profile() {
    let mut job = simple_job(vec![checkout_step(), acquire_step(), task_step(0)]);
    job.runs_on = "scale-set:velnor+orbstack-linux".to_owned();
    let jobs = BTreeMap::from([("rust-demo".to_owned(), job)]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("an arbitrary self-hosted selector does not prove linux/amd64")
            .to_string()
            .contains("declared_task_requires_supported_linux_runner")
    );
}

#[test]
fn scale_set_task_rejects_a_non_linux_profile_or_different_resolved_token() {
    let selector = scale_set_selector();
    let linux = verification_policy(VerificationRunner::LinuxX64);
    assert!(super::supports_task_runner(
        "task-rust-demo",
        "scale-set:velnor+ubuntu-26.04-scale-set",
        std::slice::from_ref(&linux),
        Some(&selector),
    ));

    let macos = verification_policy(VerificationRunner::MacosArm64);
    assert!(!super::supports_task_runner(
        "task-rust-demo",
        "scale-set:velnor+ubuntu-26.04-scale-set",
        std::slice::from_ref(&macos),
        Some(&selector),
    ));
    assert!(!super::supports_task_runner(
        "task-rust-demo",
        "scale-set:velnor+orbstack-linux",
        std::slice::from_ref(&linux),
        Some(&selector),
    ));
}

#[test]
fn crate_obligation_scale_set_uses_the_explicit_profile_without_a_task_policy() {
    let selector = scale_set_selector();
    let token = selector.token();
    let mut job = simple_job(vec![checkout_step(), acquire_step(), task_step(0)]);
    job.runs_on.clone_from(&token);
    let jobs = BTreeMap::from([("rust-demo".to_owned(), job)]);

    assert!(factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None).is_err());
    let (_, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], Some(&selector))
            .expect("validated execution profile authorizes the crate route");
    assert_eq!(files.len(), 2);
}

#[test]
fn crate_obligation_scale_set_rejects_a_different_resolved_profile() {
    let selector = scale_set_selector();
    let mut job = simple_job(vec![checkout_step(), acquire_step(), task_step(0)]);
    job.runs_on = "scale-set:velnor+orbstack-linux".to_owned();
    let jobs = BTreeMap::from([("rust-demo".to_owned(), job)]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], Some(&selector)).is_err()
    );
}

fn scale_set_selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["ubuntu-26.04-scale-set".to_owned(), "velnor".to_owned()],
    )
    .expect("validated Linux scale set")
}

fn verification_policy(runner: VerificationRunner) -> WorkflowTaskPolicy {
    WorkflowTaskPolicy::Verification(VerificationTaskPolicy {
        task: VerificationTask {
            id: "rust-demo".to_owned(),
            mise_task: "lint-demo".to_owned(),
            source: MiseTaskSource {
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
            version: "2026.10.7".to_owned(),
            sha256: "a".repeat(64),
        },
        selected_tools: Vec::new(),
        mise_config_sha256: None,
        mise_lock_sha256: None,
        rust_toolchain_sha256: None,
    })
}

fn task_document_for_test() -> Yaml {
    let task = task_step(0);
    let StepKind::TaskExecution {
        report_helper_version,
        ..
    } = &task.kind
    else {
        unreachable!();
    };
    let shape = super::Shape {
        helper_version: report_helper_version.clone(),
    };
    super::declared_task_document(0, &shape, VERSION).expect("typed composite document")
}

fn action_run_scalar(action: &Yaml) -> &str {
    let Yaml::Map(action_fields) = action else {
        panic!("composite action is a mapping");
    };
    let Some((_, Yaml::Map(runs_fields))) = action_fields.iter().find(|(key, _)| key == "runs")
    else {
        panic!("composite action has a runs mapping");
    };
    let Some((_, Yaml::Seq(steps))) = runs_fields.iter().find(|(key, _)| key == "steps") else {
        panic!("composite runs has a steps sequence");
    };
    let Some(Yaml::Map(step_fields)) = steps.first() else {
        panic!("composite action has one typed run step");
    };
    let Some((_, Yaml::Str(run))) = step_fields.iter().find(|(key, _)| key == "run") else {
        panic!("composite step has a run scalar");
    };
    run
}

fn checkout_step() -> Step {
    crate::steps::checkout_step(CHECKOUT).expect("configured checkout")
}

fn acquire_step() -> Step {
    acquire_step_for(VERSION)
}

fn acquire_step_for(version: &str) -> Step {
    let helper = format!("{}{version}", crate::steps::STAGED_BINARY_PREFIX);
    crate::steps::acquire_velnor_step(
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {helper} && echo \"$VELNOR_ASSET_SHA256  {helper}\" | sha256sum -c - && chmod +x {helper}"
            ),
        ],
        &BTreeMap::from([
            (
                crate::steps::ASSET_URL_ENV.to_owned(),
                "https://example.invalid/velnor".to_owned(),
            ),
            (crate::steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
            (crate::steps::RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
        ]),
    )
    .expect("digest-verified helper staging")
}

fn task_step(index: usize) -> Step {
    let task_id = format!("stack/rust/crate-{index}/test/default");
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["rust@1.99.0".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let mut argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        "rust@1.99.0".to_owned(),
        "--".to_owned(),
        "cargo".to_owned(),
        "test".to_owned(),
        "-p".to_owned(),
        format!("crate-{index}"),
    ];
    if index == 0 {
        argv[10] = HOSTILE_TASK_ARGUMENT.to_owned();
    }
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain id");
    let task_digest = task_digest_for_execution(&task_id, &argv, &toolchain).expect("task digest");
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group("rust", &task_id).expect("matrix id");
    let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.99.0".to_owned()),
    ]);
    let condition = velnor_actions_contract::workflow::step::task_execution_condition(&task_id)
        .expect("coverage condition");
    Step {
        name: format!("Test crate {index}"),
        id: None,
        role: None,
        condition: Some(condition),
        kind: StepKind::TaskExecution {
            argv,
            env,
            task_id,
            task_digest,
            toolchain_inputs,
            matrix_id,
            matrix_key,
            report_helper_version: VERSION.to_owned(),
            matrix_max_parallel: None,
        },
    }
}

fn simple_job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::new(20).expect("valid timeout"),
        needs: vec!["plan".to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps,
    }
}
