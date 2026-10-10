//! Obligation-step contract tests.
//!
//! Declared via `#[path]` from `matrix_step.rs` under `cfg(test)`.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
};

use super::*;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, PrepareRustComponents};
use velnor_actions_rust::{
    CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner, cargo_payload_env,
};

/// Obligation fixture for step construction.
fn obligation() -> CrateObligation {
    let task_id = "stack/rust/demo/clippy/default".to_owned();
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group("rust", &task_id)
        .expect("matrix identity");
    let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let catalog = ToolCatalog::pinned();
    let run = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        ["clippy"].into_iter().map(OsString::from).collect(),
    )
    .expect("pinned task")
    .argv(&catalog)
    .into_iter()
    .map(|word| word.into_string().expect("UTF-8 task argv"))
    .collect::<Vec<_>>();
    let toolchain_inputs = ToolchainInputs {
        tools: catalog.tool_specs(&[PinnedTool::Rust]),
        components: PrepareRustComponents::components(),
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain identity");
    CrateObligation {
        task_id: task_id.clone(),
        kind: "clippy".to_owned(),
        step_name: "Clippy".to_owned(),
        gated_by: Vec::new(),
        matrix_key,
        task_digest: task_digest_for_execution(&task_id, &run, &toolchain).expect("task digest"),
        toolchain_inputs,
        run,
    }
}

fn refresh_task_identity(obligation: &mut CrateObligation) {
    let stack = if obligation.task_id.starts_with("stack/tofu/") {
        let catalog = ToolCatalog::pinned();
        obligation.toolchain_inputs = ToolchainInputs {
            tools: catalog.tool_specs(&[PinnedTool::Opentofu]),
            components: Vec::new(),
            compile_driver: "tofu".to_owned(),
            test_runner: "tofu".to_owned(),
        };
        obligation.run = PinnedToolExec::new(
            vec![PinnedTool::Opentofu],
            OsStr::new("tofu"),
            ["-chdir", "dir-", "validate"]
                .into_iter()
                .map(OsString::from)
                .collect(),
        )
        .expect("pinned tofu task")
        .argv(&catalog)
        .into_iter()
        .map(|word| word.into_string().expect("UTF-8 task argv"))
        .collect();
        "tofu"
    } else {
        "rust"
    };
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group(stack, &obligation.task_id)
        .expect("matrix id");
    obligation.matrix_key =
        velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let toolchain = toolchain_id(&obligation.toolchain_inputs).expect("toolchain identity");
    obligation.task_digest =
        task_digest_for_execution(&obligation.task_id, &obligation.run, &toolchain)
            .expect("task digest");
}

#[test]
fn identity_env_contract_enforces_in_every_build() {
    let task_id = "stack/rust/demo/clippy/default";
    let identity = obligation_identity_env(
        task_id,
        &format!("b3-{}", "a".repeat(64)),
        "id",
        "key",
        None,
    );
    assert!(check_identity_env_contract(&identity, task_id).is_ok());
    let mut drifted = identity.clone();
    drifted.insert(
        TASK_ID_ENV.to_owned(),
        "stack/rust/demo/test/default".to_owned(),
    );
    let err = check_identity_env_contract(&drifted, task_id).expect_err("drift");
    assert!(
        err.to_string().contains("obligation_identity_mismatch"),
        "{err}"
    );
    let mut missing = identity.clone();
    missing.remove(TASK_ID_ENV);
    assert!(check_identity_env_contract(&missing, task_id).is_err());
    assert_eq!(OBLIGATION_TASK_ID_ENV, TASK_ID_ENV);
    // Log forging: a hostile task id renders as one truncated line.
    let forged = "stack/rust/demo/clippy/default\n::notice::spoofed";
    let err = check_identity_env_contract(&missing, forged).expect_err("forged");
    let text = err.to_string();
    assert!(!text.contains('\n'), "{text}");
    assert!(
        text.contains(
            "obligation_identity_mismatch:stack/rust/demo/clippy/default::notice::spoofed"
        ),
        "{text}"
    );
}

#[test]
fn obligation_step_carries_the_report_lookup_key() {
    let obligation = obligation();
    let step = obligation_step(
        &obligation,
        &ToolCatalog::pinned(),
        &[],
        None,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("step");
    let velnor_actions_contract::StepKind::TaskExecution {
        task_id,
        task_digest,
        argv,
        ..
    } = &step.kind
    else {
        panic!("obligation must be a typed task step");
    };
    assert_eq!(task_id, &obligation.task_id);
    assert_eq!(task_digest, &obligation.task_digest);
    assert_eq!(argv, &obligation.run);
}

#[test]
fn generated_pinned_obligation_renders_through_shared_declared_task_action() {
    let checkout_uses = format!("actions/checkout@{:040x}", 0);
    let task_step = generated_pinned_task_step();
    let ir = obligation_workflow_ir(&checkout_uses, task_step);
    let context = renderer_context(checkout_uses);
    let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )
    .expect("render generated workflow");
    assert!(
        yaml.contains("uses: ./.github/actions/declared-task-0"),
        "generated obligation should use the shared action:\n{yaml}"
    );
    assert!(
        yaml.contains("argv_10: demo"),
        "task argv preserved:\n{yaml}"
    );
    assert!(
        yaml.contains("task_id: stack/rust/demo/clippy/default"),
        "report identity preserved as input:\n{yaml}"
    );
}

#[test]
fn actual_nextest_producer_selector_passes_task_execution_contract() {
    let catalog = ToolCatalog::pinned();
    let task = velnor_actions_rust::propose_task(&TaskGroup {
        task_id: "stack/rust/demo/nextest/default".to_owned(),
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Nextest,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Mbx,
        test_runner: TestRunner::CargoNextest,
        nextest_profile: NextestProfile::Default,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    })
    .expect("actual Rust task proposal");
    let run = crate::vectors::task_argv(&task, &catalog).expect("catalog-backed task argv");
    let nextest_spec = catalog.tool_spec(PinnedTool::Nextest);
    assert_eq!(
        nextest_spec,
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.148"
    );
    assert!(run.contains(&nextest_spec), "producer argv: {run:?}");

    let toolchain_inputs = crate::internal_plan::identities::toolchain_inputs_for(&task, &catalog)
        .expect("catalog-backed toolchain identity");
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain digest");
    let task_digest =
        task_digest_for_execution(&task.task_id, &run, &toolchain).expect("task digest");
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group("rust", &task.task_id)
        .expect("matrix identity");
    let obligation = CrateObligation {
        task_id: task.task_id.clone(),
        kind: task.task_kind.clone(),
        step_name: "Nextest".to_owned(),
        gated_by: Vec::new(),
        matrix_key: velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key"),
        task_digest,
        toolchain_inputs,
        run,
    };
    let checkout_uses = format!("actions/checkout@{:040x}", 0);
    let step = obligation_step(&obligation, &catalog, &[], None, env!("CARGO_PKG_VERSION"))
        .expect("typed task step");
    let velnor_actions_contract::StepKind::TaskExecution {
        report_helper_version,
        ..
    } = &step.kind
    else {
        panic!("obligation must remain a typed task step");
    };
    assert_eq!(report_helper_version, env!("CARGO_PKG_VERSION"));
    let ir = obligation_workflow_ir(&checkout_uses, step);
    let context = renderer_context(checkout_uses);
    velnor_actions_workflow_renderer::render_workflow_ir(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )
    .expect("actual producer output passes the TaskExecution contract");
}

#[test]
fn actual_obligation_producer_renders_150_jobs_through_one_typed_action_shape() {
    let checkout_uses = format!("actions/checkout@{:040x}", 0);
    let mut ir = obligation_workflow_ir(&checkout_uses, generated_pinned_task_step());
    ir.jobs.remove("rust-demo");
    let catalog = ToolCatalog::pinned();
    for index in 0..150 {
        let package = format!("crate-{index}");
        let mut task = obligation();
        task.task_id = format!("stack/rust/{package}/test/default");
        task.kind = "test".to_owned();
        task.step_name = "Test".to_owned();
        task.run = PinnedToolExec::new(
            vec![PinnedTool::Rust],
            OsStr::new("cargo"),
            ["test", "-p", package.as_str()]
                .into_iter()
                .map(OsString::from)
                .collect(),
        )
        .expect("pinned Cargo test")
        .argv(&catalog)
        .into_iter()
        .map(|word| word.into_string().expect("UTF-8 task argv"))
        .collect();
        refresh_task_identity(&mut task);
        let step = obligation_step(&task, &catalog, &[], None, env!("CARGO_PKG_VERSION"))
            .expect("producer step");
        ir.jobs.insert(
            format!("rust-crate-{index}"),
            obligation_task_job(&checkout_uses, step),
        );
    }

    let context = renderer_context(checkout_uses);
    let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )
    .expect("render 150 producer-backed obligations");

    assert_eq!(
        yaml.matches("uses: ./.github/actions/declared-task-0")
            .count(),
        150,
        "each generated obligation remains a job and calls the shared action"
    );
    for index in 0..150 {
        assert!(yaml.contains(&format!("task_id: stack/rust/crate-{index}/test/default")));
        assert!(yaml.contains(&format!("crate-{index}")));
    }
}

fn generated_pinned_task_step() -> Step {
    let catalog = ToolCatalog::pinned();
    let pinned = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        ["test", "-p", "demo"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    )
    .expect("pinned Cargo task");
    let mut task = obligation();
    task.run = pinned
        .argv(&catalog)
        .into_iter()
        .map(|word| word.into_string().expect("UTF-8 pinned task argv"))
        .collect();
    let toolchain = toolchain_id(&task.toolchain_inputs).expect("toolchain identity");
    task.task_digest = task_digest_for_execution(&task.task_id, &task.run, &toolchain)
        .expect("updated task digest");
    obligation_step(&task, &catalog, &[], None, env!("CARGO_PKG_VERSION"))
        .expect("generated task step")
}

fn obligation_workflow_ir(checkout_uses: &str, task_step: Step) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: velnor_actions_workflow_renderer::CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: velnor_actions_workflow_renderer::CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: BTreeMap::from([
            ("plan".to_owned(), obligation_plan_job(checkout_uses)),
            (
                "rust-demo".to_owned(),
                obligation_task_job(checkout_uses, task_step),
            ),
        ]),
    }
}

fn obligation_plan_job(checkout_uses: &str) -> Job {
    Job {
        check_runner: None,
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checked_out(checkout_uses),
            velnor_actions_workflow_renderer::steps::write_request_step("plan-v1")
                .expect("plan request step"),
            velnor_actions_workflow_renderer::plan_step(),
        ],
    }
}

fn obligation_task_job(checkout_uses: &str, task_step: Step) -> Job {
    Job {
        check_runner: None,
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![checked_out(checkout_uses), staged_helper_step(), task_step],
    }
}

fn staged_helper_step() -> Step {
    let version = env!("CARGO_PKG_VERSION");
    let staged = format!(
        "{}{}",
        velnor_actions_workflow_renderer::STAGED_BINARY_PREFIX,
        version
    );
    velnor_actions_workflow_renderer::provision_acquire_step(
        &velnor_actions_workflow_renderer::HelperProvenance::ReleaseAsset {
            url: "https://example.invalid/velnor".to_owned(),
            sha256: "a".repeat(64),
            commit: "b".repeat(40),
        },
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
            ),
        ],
    )
    .expect("digest-verified helper staging")
}

fn checked_out(checkout_uses: &str) -> Step {
    velnor_actions_workflow_renderer::checkout_step(checkout_uses).expect("checkout step")
}

fn renderer_context(
    checkout_uses: String,
) -> velnor_actions_workflow_renderer::render::RenderContext {
    let generator_version = env!("CARGO_PKG_VERSION").to_owned();
    velnor_actions_workflow_renderer::render::RenderContext {
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{generator_version}"),
        generator_version,
        report_helper_version: generator_version.clone(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses,
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    }
}

#[test]
fn obligation_step_skips_when_plan_covered_it() {
    let obligation = obligation();
    let step = obligation_step(
        &obligation,
        &ToolCatalog::pinned(),
        &[],
        None,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("step");
    assert_eq!(
        step.condition.as_deref(),
        Some("!contains(needs.plan.outputs.covered_tasks, ',stack/rust/demo/clippy/default,')")
    );
    let mut forged = obligation.clone();
    forged.task_id = "not-a-task".to_owned();
    assert!(
        obligation_step(
            &forged,
            &ToolCatalog::pinned(),
            &[],
            None,
            env!("CARGO_PKG_VERSION")
        )
        .expect_err("malformed id")
        .to_string()
        .contains("malformed_task_id"),
        "malformed IDs never reach generated expressions"
    );
}

#[test]
fn report_wrapper_stamps_start_and_hands_env_to_helper() {
    let argv = report_wrapper_argv("true", "/tmp/h");
    assert_eq!(&argv[..2], ["sh".to_owned(), "-c".to_owned()]);
    let script = &argv[2];
    assert!(
        script.contains("s=$(date +%s%3N); "),
        "start stamp first: {script}"
    );
    for env in [EXIT_CODE_ENV, START_MS_ENV] {
        assert!(script.contains(env), "helper env {env}: {script}");
    }
    assert!(
        script.contains(&format!("{START_MS_ENV}=\"$s\"")),
        "stamp handed to helper: {script}"
    );
    assert!(
        script.ends_with("if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""),
        "obligation code wins: {script}"
    );
    velnor_actions_workflow_renderer::validate_command_argv(&argv).expect("valid wrapper");
}

#[test]
fn outcome_and_deferred_share_one_start_file() {
    let outcome = outcome_path_for_key("m-abc");
    let start = start_path_for_key("m-abc");
    assert_eq!(outcome, "$RUNNER_TEMP/velnor/outcome-m-abc");
    assert_eq!(start, "$RUNNER_TEMP/velnor/start-m-abc");
    let save = outcome_wrapper_argv("true", &outcome, &start);
    assert!(
        save[2].starts_with(&format!("date +%s%3N > \"{start}\"; ")),
        "outcome stamps start first (no unset prelude on outcome): {}",
        save[2]
    );
    assert!(
        save[2].contains(&format!("echo \"$code\" > \"{outcome}\"")),
        "{}",
        save[2]
    );
    let report = deferred_report_argv(&outcome, "/tmp/h", &start);
    assert!(
        report[2].contains(&format!("read -r start_ms rest < \"{start}\"")),
        "deferred reads stamp: {}",
        report[2]
    );
    assert!(
        report[2].contains(&format!("{START_MS_ENV}=\"$start_ms\"")),
        "deferred hands stamp: {}",
        report[2]
    );
    for argv in [&save, &report] {
        velnor_actions_workflow_renderer::validate_command_argv(argv).expect("valid wrapper");
        assert!(!argv[2].contains("$("), "file handoff only: {}", argv[2]);
    }
}

#[test]
fn doc_obligation_step_carries_typed_rustdocflags() {
    use velnor_actions_rust::{DENY_WARNINGS, RUSTDOCFLAGS_ENV};
    let mut doc = obligation();
    doc.task_id = "stack/rust/demo/doc/default".to_owned();
    doc.kind = TaskKind::Doc.as_str().to_owned();
    doc.step_name = DOCUMENTATION_NAME.to_owned();
    refresh_task_identity(&mut doc);
    let step = obligation_step(
        &doc,
        &ToolCatalog::pinned(),
        &[],
        None,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("step");
    let velnor_actions_contract::StepKind::TaskExecution { env, .. } = &step.kind else {
        panic!("obligation must be a typed task step");
    };
    let typed: Vec<(String, String)> = cargo_payload_env(TaskKind::Doc)
        .into_iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        env.get(RUSTDOCFLAGS_ENV).map(String::as_str),
        Some(DENY_WARNINGS),
        "rendered doc step must deny warnings"
    );
    assert!(
        typed
            .iter()
            .all(|(key, value)| env.get(key).is_some_and(|seen| seen == value)),
        "rendered env must match the typed payload: {typed:?}"
    );
}

#[test]
fn tofu_obligation_steps_carry_the_automation_pair() {
    use velnor_actions_mise::{MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV, RUSTUP_TOOLCHAIN_ENV};
    use velnor_actions_tofu::{
        TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON,
        TF_INPUT_ENV, TF_INPUT_OFF,
    };
    let mut tofu = obligation();
    tofu.task_id = "stack/tofu/dir-/validate/default".to_owned();
    tofu.kind = "validate".to_owned();
    tofu.step_name = "Validate".to_owned();
    refresh_task_identity(&mut tofu);
    let step = obligation_step(
        &tofu,
        &ToolCatalog::pinned(),
        &[],
        None,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("step");
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("OpenTofu remains a shell step");
    };
    for key in [
        MISE_RUSTUP_HOME_ENV,
        MISE_CARGO_HOME_ENV,
        RUSTUP_TOOLCHAIN_ENV,
    ] {
        assert!(
            !env.contains_key(key),
            "tofu obligations carry no owned-homes triple"
        );
    }
    assert_eq!(
        env.get(TF_IN_AUTOMATION_ENV).map(String::as_str),
        Some(TF_IN_AUTOMATION_ON),
        "tofu steps mark automation"
    );
    assert_eq!(
        env.get(TF_INPUT_ENV).map(String::as_str),
        Some(TF_INPUT_OFF),
        "tofu steps disable input"
    );
    assert_eq!(
        env.get(TF_DATA_DIR_ENV).map(String::as_str),
        Some(
            velnor_actions_tofu::tofu_data_dir_under(
                velnor_actions_mise::runtime_paths::TOFU_DATA_BASE_EXPR,
                "",
            )
            .expect("data dir")
            .as_str(),
        ),
        "tofu steps isolate the per-root data dir"
    );
    assert!(
        !env.contains_key(TF_CLI_CONFIG_FILE_ENV),
        "temp CLI config stays local-only until a materialization step lands"
    );
    let step = obligation_step(
        &obligation(),
        &ToolCatalog::pinned(),
        &[],
        None,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("step");
    let velnor_actions_contract::StepKind::TaskExecution { env, .. } = &step.kind else {
        panic!("obligation must be a typed task step");
    };
    assert!(
        !env.contains_key(TF_DATA_DIR_ENV),
        "rust steps carry no tofu data dir"
    );
    for key in [
        MISE_RUSTUP_HOME_ENV,
        MISE_CARGO_HOME_ENV,
        RUSTUP_TOOLCHAIN_ENV,
    ] {
        assert!(
            env.get(key).is_some_and(|value| !value.is_empty()),
            "rust steps keep the owned-homes triple"
        );
    }
}

#[test]
fn non_doc_obligation_steps_carry_no_rustdocflags() {
    use velnor_actions_rust::RUSTDOCFLAGS_ENV;
    let step = obligation_step(
        &obligation(),
        &ToolCatalog::pinned(),
        &[],
        None,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("step");
    let velnor_actions_contract::StepKind::TaskExecution { env, .. } = &step.kind else {
        panic!("Rust obligations use typed task steps");
    };
    assert!(
        !env.contains_key(RUSTDOCFLAGS_ENV),
        "clippy must not carry doc env"
    );
}

#[test]
fn validator_installs_follow_executed_suite_per_policy() {
    use velnor_actions_contract::WorkflowPolicy;
    for package in [
        "velnor-actions-orchestrator",
        "velnor-actions-cli",
        "velnor-actions-contract",
        "velnor-actions-mise",
        "demo",
        "velnor-actions-freshness",
    ] {
        assert!(
            crate_needs_generate_validators(WorkflowPolicy::ConsumerV1, suite_for_package(package)),
            "consumer suites are opaque: {package} keeps the trio"
        );
    }
    for package in ["velnor-actions-orchestrator", "velnor-actions-cli"] {
        assert!(
            crate_needs_generate_validators(
                WorkflowPolicy::VelnorRepositoryV1,
                suite_for_package(package)
            ),
            "{package} spawns validators and must install them"
        );
    }
    for package in [
        "velnor-actions-contract",
        "velnor-actions-mise",
        "velnor-actions-rust",
        "velnor-actions-tofu",
        "velnor-actions-workflow-renderer",
        "velnor-actions-actionlint",
        "velnor-actions-freshness",
        "demo",
    ] {
        assert!(
            !crate_needs_generate_validators(
                WorkflowPolicy::VelnorRepositoryV1,
                suite_for_package(package)
            ),
            "{package} never spawns validators and must trim the trio"
        );
    }
}

/// Every workspace member is classified: validator-spawning or trimmed.
///
/// A new crate fails here by name until its suite is audited for
/// trio execution (see `SUITE_TOOL_OWNERS`) and classified.
#[test]
fn every_workspace_member_is_classified() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crate lives two levels under the workspace root");
    let manifest: toml::Value = toml::from_str(
        &std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest reads"),
    )
    .expect("workspace manifest parses");
    let members = manifest["workspace"]["members"]
        .as_array()
        .expect("workspace members are explicit paths");
    assert!(!members.is_empty(), "workspace must declare members");
    let members: Vec<String> = members
        .iter()
        .map(|member| {
            let path = member.as_str().expect("workspace member is a path");
            let manifest: toml::Value = toml::from_str(
                &std::fs::read_to_string(root.join(path).join("Cargo.toml"))
                    .expect("member manifest reads"),
            )
            .expect("member manifest parses");
            manifest["package"]["name"]
                .as_str()
                .expect("workspace member declares its package name")
                .to_owned()
        })
        .collect();
    for member in &members {
        let known = [
            "velnor-actions-orchestrator",
            "velnor-actions-cli",
            "velnor-actions-contract",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-tofu",
            "velnor-actions-workflow-renderer",
            "velnor-actions-actionlint",
            "velnor-actions-freshness",
            "velnor-archive-guard",
        ]
        .contains(&member.as_str());
        assert!(
            known,
            "{member} is unclassified: audit its suite for trio execution, then classify it"
        );
    }
}
