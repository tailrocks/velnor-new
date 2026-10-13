use super::*;

#[test]
fn policy_vectors_pin_specs_and_payloads() {
    let roots = [String::new(), "crates/velnor-runner".to_owned()];
    let deny = deny_argv(&roots).expect("deny argv");
    assert_eq!(&deny[..2], ["sh", "-c"], "deny runs isolated, not bare");
    let script = &deny[2];
    for need in [
        "mise --no-config --no-env --no-hooks install cargo-deny@0.20.2",
        "unset ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "MISE_GITHUB_TOKEN",
        "mkdir -p \"$RUNNER_TEMP/velnor/cargo-clean\"",
        "cd \"$RUNNER_TEMP/velnor/cargo-clean\"",
        "mise --no-config --no-env --no-hooks exec cargo-deny@0.20.2 -- cargo deny --locked",
        "--manifest-path \"$GITHUB_WORKSPACE/Cargo.toml\"",
        "--config \"$GITHUB_WORKSPACE/deny.toml\" check",
        "--manifest-path \"$GITHUB_WORKSPACE/crates/velnor-runner/Cargo.toml\"",
        "--config \"$GITHUB_WORKSPACE/crates/velnor-runner/deny.toml\" check",
    ] {
        assert!(script.contains(need), "deny script misses {need}: {script}");
    }
    let at = |needle: &str| {
        script
            .find(needle)
            .unwrap_or_else(|| panic!("deny script misses {needle}: {script}"))
    };
    assert!(
        at("mise --no-config --no-env --no-hooks install") < at("unset ")
            && at("unset ") < at("cargo deny"),
        "deny must bootstrap, then drop creds, then run cargo: {script}"
    );
    assert!(
        at("$GITHUB_WORKSPACE/Cargo.toml\" --config")
            < at("$GITHUB_WORKSPACE/crates/velnor-runner/Cargo.toml\" --config"),
        "both workspaces must be checked in declared order: {script}"
    );
    let root_only = deny_argv(&[String::new()]).expect("root-only deny argv");
    assert!(root_only[2].contains("$GITHUB_WORKSPACE/Cargo.toml"));
    assert!(
        !root_only[2].contains("velnor-runner"),
        "discovery without a nested runner must not inject its manifest"
    );
    let machete = machete_argv().expect("machete argv");
    let want = argv_of(&[
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "ubi:bnjbvr/cargo-machete@0.9.2",
        "--",
        "cargo",
        "machete",
        "crates/core/velnor-actions-contract",
        "crates/adapters/velnor-actions-rust",
        "crates/adapters/velnor-actions-tofu",
        "crates/adapters/velnor-actions-mise",
        "crates/adapters/velnor-actions-actionlint",
        "crates/services/velnor-actions-workflow-renderer",
        "crates/services/velnor-actions-orchestrator",
        "crates/apps/velnor-actions-cli",
    ]);
    assert_eq!(machete, want);
    assert!(validator_argv("evil-tool", "1.2.3", "cargo", &["deny"]).is_err());
    assert!(validator_argv("cargo-deny", "latest", "cargo", &["deny"]).is_err());
}

#[test]
fn mbx_probe_vector_is_byte_exact() {
    let probe = mbx_probe_argv(&ToolCatalog::pinned()).expect("probe argv");
    let want = argv_of(&[
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.98.1",
        "--",
        "mbx",
        "--version",
    ]);
    assert_eq!(probe, want);
}

/// Minimal proposal with one compile driver.
fn group_with_driver(driver: velnor_actions_rust::CompileDriver) -> ProposedTask {
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
        compile_driver: driver,
        test_runner: velnor_actions_rust::TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn task_payload_program_follows_route_driver() {
    let catalog = ToolCatalog::pinned();
    let rust_spec = catalog.tool_spec(PinnedTool::Rust);
    for (driver, program) in [(CompileDriver::Cargo, "cargo"), (CompileDriver::Mbx, "mbx")] {
        let argv = task_argv(&group_with_driver(driver), &catalog).expect("task argv");
        let at = argv.iter().position(|arg| arg == "--").expect("separator");
        assert_eq!(argv[at + 1], program, "{} program", driver.as_str());
        assert_eq!(
            &argv[5..at],
            std::slice::from_ref(&rust_spec),
            "{} selects only Rust through Mise; MBX is action-owned",
            driver.as_str()
        );
        assert!(
            !argv.iter().any(|arg| arg.contains("mr-boxington")),
            "{} must not reinstall action-owned MBX: {argv:?}",
            driver.as_str()
        );
    }
}

#[test]
fn task_runner_tools_follow_test_runner() {
    let catalog = ToolCatalog::pinned();
    let nextest = catalog.tool_spec(PinnedTool::Nextest);
    for (runner, want) in [
        (TestRunner::CargoTest, false),
        (TestRunner::CargoNextest, true),
    ] {
        let mut task = group_with_driver(CompileDriver::Cargo);
        task.identity.test_runner = runner.as_str().to_owned();
        let argv = task_argv(&task, &catalog).expect("task argv");
        assert_eq!(argv.contains(&nextest), want, "{} nextest", runner.as_str());
    }
}

#[test]
fn zizmor_vector_is_pinned_and_offline() {
    let argv = zizmor_argv(&ToolCatalog::pinned()).expect("zizmor argv");
    assert_eq!(
        argv.join(" "),
        "mise --no-config --no-env --no-hooks exec zizmor@1.30.1 -- zizmor \
             --no-online-audits --config .zizmor.yml .github/workflows"
    );
}

#[test]
fn section4_build_vector_is_byte_exact() {
    let build = candidate_build_argv(&ToolCatalog::pinned()).expect("build argv");
    let want = argv_of(&[
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.98.1",
        "--",
        "mbx",
        "build",
        "--release",
        "--locked",
        "--package",
        "velnor-actions-cli",
        "--bin",
        "velnor-actions",
    ]);
    assert_eq!(build, want);
}

#[test]
fn tofu_task_argv_routes_through_pinned_opentofu() {
    use velnor_actions_tofu_core::{TofuTaskGroup, TofuTaskKind};
    let group = TofuTaskGroup {
        root: String::new(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    let catalog = ToolCatalog::pinned();
    let argv = task_argv(&task, &catalog).expect("task argv");
    assert!(
        argv.contains(&catalog.tool_spec(PinnedTool::Opentofu)),
        "opentofu spec: {argv:?}"
    );
    assert!(
        !argv
            .iter()
            .any(|arg| arg == "cargo" || arg.contains("nextest")),
        "no rust tools: {argv:?}"
    );
    let at = argv.iter().position(|arg| arg == "--").expect("separator");
    // Program `tofu` plus the fixed payload, wrapped never edited.
    assert_eq!(&argv[at + 1..], ["tofu", "validate", "-no-color"]);
}

#[test]
fn tofu_subdir_payload_runs_under_chdir_first() {
    use velnor_actions_tofu_core::{TofuTaskGroup, TofuTaskKind};
    let group = TofuTaskGroup {
        root: "stacks/a".to_owned(),
        kind: TofuTaskKind::Fmt,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    let catalog = ToolCatalog::pinned();
    let argv = task_argv(&task, &catalog).expect("task argv");
    let at = argv.iter().position(|arg| arg == "--").expect("separator");
    assert_eq!(
        &argv[at + 1..],
        [
            "tofu",
            "-chdir",
            "stacks/a",
            "fmt",
            "-check",
            "-recursive",
            "-no-color"
        ]
    );
}

#[test]
fn candidate_build_delegates_to_mise_constructor() {
    let catalog = ToolCatalog::pinned();
    let mine = candidate_build_argv(&catalog).expect("build argv");
    let owned = CandidateBuild::new()
        .expect("mise build")
        .argv(&catalog)
        .into_iter()
        .map(|arg| arg.into_string().expect("utf8"))
        .collect::<Vec<_>>();
    assert_eq!(mine, owned);
}
