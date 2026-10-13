use super::*;
use crate::task_report::{EXIT_CODE_ENV, START_MS_ENV};

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
    assert_tofu_step_automation_environment();
    assert_rust_step_owned_homes();
}

fn assert_tofu_step_automation_environment() {
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
}

fn assert_rust_step_owned_homes() {
    use velnor_actions_mise::{MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV, RUSTUP_TOOLCHAIN_ENV};
    use velnor_actions_tofu::TF_DATA_DIR_ENV;
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
