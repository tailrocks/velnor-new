use super::*;

#[test]
fn compiler_inventory_denies_formatting_workloads_and_unknown_kinds() {
    for kind in ["clippy", "test", "nextest", "doctest", "doc", "build"] {
        assert!(requires_compiler_task(&format!(
            "stack/rust/root/{kind}/default"
        )));
    }
    for task in [
        "stack/rust/root/fmt/default",
        "stack/rust/root/custom/default",
        "stack/workload/root/build/default",
        "stack/tofu/root/init/default",
    ] {
        assert!(!requires_compiler_task(task));
    }
}

#[test]
fn original_recipe_rejects_edited_report_identity_and_command() {
    let recipe = fixture_recipe();
    let obligation = fixture_obligation(&recipe);
    assert!(recipe.validate_obligation(&obligation).is_ok());
    let mut digest = obligation.clone();
    digest.task_digest =
        "b3-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned();
    assert!(recipe.validate_obligation(&digest).is_err());
    let mut command = obligation.clone();
    command.run.push("--all-features".to_owned());
    assert!(recipe.validate_obligation(&command).is_err());
    let mut key = obligation;
    key.matrix_key = "other".to_owned();
    assert!(recipe.validate_obligation(&key).is_err());
}

#[test]
fn proposal_selection_skips_fmt_and_rejects_unknown_driver() {
    let catalog = ToolCatalog::pinned();
    let mut task = fixture_recipe().task;
    task.identity.compile_driver = "cargo custom".to_owned();
    assert!(RustReportWrapper::from_proposal(&task, &catalog, "ubuntu-24.04").is_err());
    task.identity.compile_driver = "cargo".to_owned();
    task.identity.test_runner = "arbitrary-runner".to_owned();
    assert!(RustReportWrapper::from_proposal(&task, &catalog, "ubuntu-24.04").is_err());
    task.task_id = "stack/rust/root/fmt/default".to_owned();
    task.task_kind = "fmt".to_owned();
    assert!(
        RustReportWrapper::from_proposal(&task, &catalog, "ubuntu-24.04")
            .expect("noncompiler selection")
            .is_none()
    );
}

#[test]
fn report_environment_binds_digest_and_preserves_rust_payload_flags() {
    let mut recipe = fixture_recipe();
    recipe.task.task_kind = "doc".to_owned();
    recipe.task.task_id = "stack/rust/root/doc/default".to_owned();
    let obligation = fixture_obligation(&recipe);
    let downstream = vec!["stack/rust/root/build/default".to_owned()];
    let environment = recipe
        .environment(&obligation, &ToolCatalog::pinned(), &downstream, Some(3))
        .expect("report environment");
    assert_eq!(environment.get("VELNOR_TASK_DIGEST"), Some(&recipe.digest));
    assert_eq!(
        environment.get("VELNOR_TASK_ID"),
        Some(&recipe.task.task_id)
    );
    assert_eq!(
        environment.get("RUSTDOCFLAGS").map(String::as_str),
        Some("-D warnings")
    );
    assert_eq!(
        environment
            .get(crate::task_report::DOWNSTREAM_IDS_ENV)
            .map(String::as_str),
        Some(downstream[0].as_str())
    );
}

#[test]
fn literal_frame_assignment_preserves_exact_json_in_minimal_shell() {
    let argv = vec![
        "cargo",
        "clippy",
        "$RUNNER_TEMP/owner's compiler",
        "$(touch guard-marker)",
        "`touch guard-marker`",
    ];
    let json = serde_json::to_string(&argv).expect("frame JSON");
    let assignment = format!(
        "VELNOR_RUST_FRAME_ARGV_JSON={}",
        velnor_actions_contract::quote_literal_run_arg(&json)
    );
    assert_eq!(
        assignment,
        r#"VELNOR_RUST_FRAME_ARGV_JSON='["cargo","clippy","$RUNNER_TEMP/owner'\''s compiler","$(touch guard-marker)","`touch guard-marker`"]'"#
    );
    let output = std::process::Command::new("/bin/sh")
        .env_clear()
        .arg("-c")
        .arg(format!(
            "{assignment}; printf '%s' \"$VELNOR_RUST_FRAME_ARGV_JSON\""
        ))
        .output()
        .expect("minimal shell quote proof");
    assert!(output.status.success());
    assert_eq!(output.stdout, json.as_bytes());
    assert_eq!(
        serde_json::from_slice::<Vec<String>>(&output.stdout).expect("exact JSON"),
        argv
    );
}

#[test]
fn compiler_inventory_and_frame_support_nested_sharded_task_ids() {
    let mut recipe = fixture_recipe();
    recipe.task.task_id = "stack/rust/nested/library/nextest/default/shard-1-of-2".to_owned();
    recipe.task.task_kind = "nextest".to_owned();
    assert!(requires_compiler_task(&recipe.task.task_id));
    let obligation = fixture_obligation(&recipe);
    assert!(recipe.validate_obligation(&obligation).is_ok());
}

fn fixture_recipe() -> RustReportWrapper {
    let group = velnor_actions_rust::TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: "root".to_owned(),
        kind: velnor_actions_rust::TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: velnor_actions_rust::CompileDriver::Cargo,
        test_runner: velnor_actions_rust::TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
    };
    RustReportWrapper {
        task: velnor_actions_rust::propose_task(&group).expect("adapter proposal"),
        argv: vec!["cargo".to_owned(), "clippy".to_owned()],
        toolchain: "qualified-toolchain".to_owned(),
        digest: "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        driver: RouteDriver::Cargo,
    }
}

fn fixture_obligation(recipe: &RustReportWrapper) -> CrateObligation {
    let id =
        velnor_actions_contract::matrix_id_for_task_group(Stack::Rust.id(), &recipe.task.task_id)
            .expect("matrix id");
    CrateObligation {
        task_id: recipe.task.task_id.clone(),
        kind: recipe.task.task_kind.clone(),
        step_name: crate::matrix_step::step_name_for(&recipe.task.task_kind, &recipe.task.task_id),
        gated_by: Vec::new(),
        matrix_key: velnor_actions_contract::matrix_key_for_id(&id).expect("matrix key"),
        task_digest: recipe.digest.clone(),
        run: recipe.argv.clone(),
    }
}
