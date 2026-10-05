//! Exact task-root binding for generated OpenTofu report wrappers.
use super::*;

const TEST_OPENTOFU_VERSION: &str = "1.13.1";

fn task_matrix_id(task_id: &str) -> String {
    matrix_id_for_task_group("tofu", task_id).expect("valid tofu matrix id")
}

fn task_matrix_key(task_id: &str) -> String {
    matrix_key_for_id(&task_matrix_id(task_id)).expect("valid tofu matrix key")
}

fn task_env(task_id: &str) -> BTreeMap<String, String> {
    let matrix_id = task_matrix_id(task_id);
    let matrix_key = matrix_key_for_id(&matrix_id).expect("valid tofu matrix key");
    let env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_DATA_DIR".to_owned(),
            "${{ github.workspace }}/.velnor/cache/mise".to_owned(),
        ),
        ("TF_IN_AUTOMATION".to_owned(), "1".to_owned()),
        ("TF_INPUT".to_owned(), "0".to_owned()),
        ("VELNOR_TASK_ID".to_owned(), task_id.to_owned()),
        ("VELNOR_TASK_DIGEST".to_owned(), TASK_DIGEST.to_owned()),
        ("VELNOR_MATRIX_ID".to_owned(), matrix_id),
        ("VELNOR_MATRIX_KEY".to_owned(), matrix_key),
    ]);
    env
}

fn report_step(task_id: &str, name: &str, payload: &[String]) -> Step {
    let mut argv = ["mise", "--no-config", "--no-env", "--no-hooks", "exec"]
        .map(str::to_owned)
        .to_vec();
    argv.extend([format!("opentofu@{TEST_OPENTOFU_VERSION}"), "--".to_owned()]);
    argv.extend(payload.iter().cloned());
    let joined = join_argv_for_run(&argv).expect("typed OpenTofu argv joins");
    let matrix_key = task_matrix_key(task_id);
    let mut step = shell_step(
        name,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            report_body(&joined, &matrix_key),
        ],
        task_env(task_id),
    )
    .expect("captured OpenTofu report wrapper is valid");
    step.condition = Some(format!(
        "!contains(needs.plan.outputs.{}, ',{task_id},')",
        velnor_actions_workflow_renderer::COVERED_TASKS_OUTPUT
    ));
    step
}

fn tofu_payload(root: &str, kind: &str) -> Vec<String> {
    let mut argv = vec!["tofu".to_owned()];
    if !root.is_empty() {
        argv.extend(["-chdir".to_owned(), root.to_owned()]);
    }
    match kind {
        "fmt" => argv.extend(["fmt", "-check", "-recursive", "-no-color"].map(str::to_owned)),
        "init" => argv.extend(
            [
                "init",
                "-backend=false",
                "-input=false",
                "-lockfile=readonly",
                "-no-color",
            ]
            .map(str::to_owned),
        ),
        "validate" => argv.extend(["validate", "-no-color"].map(str::to_owned)),
        _ => panic!("test uses a known typed OpenTofu task"),
    }
    argv
}

fn assert_report_tools(task_id: &str, name: &str, payload: &[String]) {
    let step = report_step(task_id, name, payload);
    let StepKind::Shell { env, .. } = &step.kind else {
        panic!("OpenTofu report wrapper must be a shell step");
    };
    assert_eq!(
        env.get("MISE_DATA_DIR").map(String::as_str),
        Some("${{ github.workspace }}/.velnor/cache/mise")
    );
    for key in [
        "MISE_RUSTUP_HOME",
        "RUSTUP_HOME",
        "MISE_CARGO_HOME",
        "CARGO_HOME",
        "RUSTUP_TOOLCHAIN",
    ] {
        assert!(!env.contains_key(key), "pure OpenTofu omits {key}");
    }
    let (job_id, mut workflow_job) = job(
        "tofu-report",
        "OpenTofu report wrapper",
        Vec::new(),
        vec![step],
    );
    assert_eq!(
        infer_job_tools(&workflow_job),
        [format!("opentofu@{TEST_OPENTOFU_VERSION}")]
    );
    assert!(
        ensure_setup_p08(
            &job_id,
            &mut workflow_job,
            &mise(),
            false,
            "x86_64-unknown-linux-gnu"
        )
        .is_ok(),
        "exact generated OpenTofu task is a valid typed tool source"
    );
}

fn assert_rejected(step: Step, id: &str) {
    assert_setup_rejects(step, id);
}

#[test]
fn generated_root_and_nested_opentofu_tasks_match_exact_payloads() {
    assert_report_tools(
        "stack/tofu/root/fmt/default",
        "Format",
        &tofu_payload("", "fmt"),
    );
    assert_report_tools(
        "stack/tofu/stacks/a/init/default",
        "Init for validate",
        &tofu_payload("stacks/a", "init"),
    );
    assert_report_tools(
        "stack/tofu/stacks/a/validate/default",
        "Validate",
        &tofu_payload("stacks/a", "validate"),
    );
}

#[test]
fn generated_opentofu_wrapper_rejects_root_subcommand_and_flag_changes() {
    let task_id = "stack/tofu/stacks/a/init/default";
    assert_rejected(
        report_step(
            task_id,
            "Init for validate",
            &tofu_payload("stacks/b", "init"),
        ),
        "tofu-report-root-mutated",
    );
    assert_rejected(
        report_step(
            task_id,
            "Init for validate",
            &tofu_payload("stacks/a", "validate"),
        ),
        "tofu-report-subcommand-mutated",
    );
    let mut changed_flags = tofu_payload("stacks/a", "init");
    let readonly = changed_flags
        .iter_mut()
        .find(|arg| arg.as_str() == "-lockfile=readonly")
        .expect("read-only lock flag");
    *readonly = "-lockfile=update".to_owned();
    assert_rejected(
        report_step(task_id, "Init for validate", &changed_flags),
        "tofu-report-flags-mutated",
    );
}
