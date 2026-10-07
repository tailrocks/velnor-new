//! Obligation-step contract tests.
//!
//! Declared via `#[path]` from `matrix_step.rs` under `cfg(test)`.

use super::*;

use velnor_actions_rust::{TaskKind, cargo_payload_env};

/// Obligation fixture for step construction.
fn obligation() -> CrateObligation {
    CrateObligation {
        task_id: "stack/rust/demo/clippy/default".to_owned(),
        kind: "clippy".to_owned(),
        step_name: "Clippy".to_owned(),
        gated_by: Vec::new(),
        matrix_key: "m-0123456789abcdef".to_owned(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        run: vec!["true".to_owned()],
    }
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
    let step = obligation_step(&obligation, &ToolCatalog::pinned(), &[], None).expect("step");
    let velnor_actions_contract_workflow::StepKind::Shell { run, env } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    assert_eq!(
        env.get(TASK_ID_ENV).map(String::as_str),
        Some(obligation.task_id.as_str())
    );
    assert!(run[2].contains(REPORT_OP), "wrapper reports: {run:?}");
}

#[test]
fn obligation_step_skips_when_plan_covered_it() {
    let obligation = obligation();
    let step = obligation_step(&obligation, &ToolCatalog::pinned(), &[], None).expect("step");
    assert_eq!(
        step.condition.as_deref(),
        Some("!contains(needs.plan.outputs.covered_tasks, ',stack/rust/demo/clippy/default,')")
    );
    let mut forged = obligation.clone();
    forged.task_id = "not-a-task".to_owned();
    assert!(
        obligation_step(&forged, &ToolCatalog::pinned(), &[], None)
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
    velnor_actions_workflow_steps::validate_command_argv(&argv).expect("valid wrapper");
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
        velnor_actions_workflow_steps::validate_command_argv(argv).expect("valid wrapper");
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
    let step = obligation_step(&doc, &ToolCatalog::pinned(), &[], None).expect("step");
    let velnor_actions_contract_workflow::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
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
    use velnor_actions_tofu_core::{
        TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON,
        TF_INPUT_ENV, TF_INPUT_OFF,
    };
    let mut tofu = obligation();
    tofu.task_id = "stack/tofu/root/validate/default".to_owned();
    tofu.kind = "validate".to_owned();
    tofu.step_name = "Validate".to_owned();
    let step = obligation_step(&tofu, &ToolCatalog::pinned(), &[], None).expect("step");
    let velnor_actions_contract_workflow::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
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
        Some("${{ runner.temp }}/velnor/tofu-data/root-af1349b9f5f9"),
        "tofu steps isolate the per-root data dir"
    );
    assert!(
        !env.contains_key(TF_CLI_CONFIG_FILE_ENV),
        "temp CLI config stays local-only until a materialization step lands"
    );
    let step = obligation_step(&obligation(), &ToolCatalog::pinned(), &[], None).expect("step");
    let velnor_actions_contract_workflow::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
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
    let step = obligation_step(&obligation(), &ToolCatalog::pinned(), &[], None).expect("step");
    let velnor_actions_contract_workflow::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    assert!(
        !env.contains_key(RUSTDOCFLAGS_ENV),
        "clippy must not carry doc env"
    );
}

#[test]
fn validator_installs_follow_executed_suite_per_policy() {
    use velnor_actions_contract_config::WorkflowPolicy;
    for package in [
        "velnor-actions-orchestrator",
        "velnor-actions-cli",
        "velnor-actions-contract",
        "velnor-actions-mise",
        "demo",
    ] {
        assert!(
            crate_needs_generate_validators(WorkflowPolicy::ConsumerV1, package),
            "consumer suites are opaque: {package} keeps the trio"
        );
    }
    for package in ["velnor-actions-orchestrator", "velnor-actions-cli"] {
        assert!(
            crate_needs_generate_validators(WorkflowPolicy::VelnorRepositoryV1, package),
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
        "demo",
    ] {
        assert!(
            !crate_needs_generate_validators(WorkflowPolicy::VelnorRepositoryV1, package),
            "{package} never spawns validators and must trim the trio"
        );
    }
}

/// Every workspace member is classified: validator-spawning or trimmed.
///
/// A new crate fails here by name until its suite is audited for
/// trio execution (see `GENERATE_VALIDATOR_SUITES`) and classified.
#[test]
fn every_workspace_member_is_classified() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .expect("crate lives three levels under the workspace root");
    let mut members = Vec::new();
    let crates = std::fs::read_dir(root.join("crates")).expect("crates dir lists");
    for entry in crates {
        let group = entry.expect("dir entry reads").path();
        if !group.is_dir() {
            continue;
        }
        let packages = std::fs::read_dir(&group).expect("group dir lists");
        for package in packages {
            let manifest = package
                .expect("package entry reads")
                .path()
                .join("Cargo.toml");
            if !manifest.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&manifest).expect("member manifest reads");
            let mut in_package = false;
            for line in text.lines() {
                if line.trim() == "[package]" {
                    in_package = true;
                } else if line.starts_with('[') {
                    in_package = false;
                } else if in_package && line.trim_start().starts_with("name = ") {
                    let name = line
                        .trim_start()
                        .trim_start_matches("name = ")
                        .trim()
                        .trim_matches('"');
                    members.push(name.to_owned());
                    break;
                }
            }
        }
    }
    assert!(!members.is_empty(), "workspace scan must find members");
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
            "velnor-archive-guard",
        ]
        .contains(&member.as_str());
        assert!(
            known,
            "{member} is unclassified: audit its suite for trio execution, then classify it"
        );
    }
}
