use super::*;

/// Owned argv expectation from literals.
fn argv_of(parts: &[&str]) -> Vec<String> {
    parts.iter().map(ToString::to_string).collect()
}

#[test]
fn policy_vectors_pin_specs_and_payloads() {
    let deny = deny_argv().expect("deny argv");
    let want = argv_of(&[
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "cargo-deny@0.20.2",
        "--",
        "cargo",
        "deny",
        "--locked",
        "check",
    ]);
    assert_eq!(deny, want);
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
        "crates/velnor-actions-contract",
        "crates/velnor-actions-rust",
        "crates/velnor-actions-mise",
        "crates/velnor-actions-actionlint",
        "crates/velnor-actions-workflow-renderer",
        "crates/velnor-actions-orchestrator",
        "crates/velnor-actions-cli",
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
        "mr-boxington@1.19.0",
        "--",
        "mbx",
        "--version",
    ]);
    assert_eq!(probe, want);
}

/// Minimal group with one compile-driver spelling.
fn group_with_driver(driver: &str) -> TaskGroup {
    TaskGroup {
        task_id: "stack/rust|task/t".to_owned(),
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
        compile_driver: driver.to_owned(),
        test_runner: "cargo_test".to_owned(),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        nextest_profile: "default".to_owned(),
    }
}

#[test]
fn task_payload_program_follows_route_driver() {
    let catalog = ToolCatalog::pinned();
    for (driver, program, mbx) in [
        ("cargo", "cargo", false),
        ("mbx", "mbx", true),
        ("bogus", "cargo", false),
    ] {
        let argv = task_argv(&group_with_driver(driver), &catalog).expect("task argv");
        let at = argv.iter().position(|arg| arg == "--").expect("separator");
        assert_eq!(argv[at + 1], program, "{driver} program");
        assert_eq!(
            argv.iter().any(|arg| arg.contains("mr-boxington")),
            mbx,
            "{driver} tools"
        );
    }
}

#[test]
fn task_runner_tools_follow_test_runner() {
    let catalog = ToolCatalog::pinned();
    let nextest = catalog.tool_spec(PinnedTool::Nextest);
    for (runner, want) in [("cargo_test", false), ("cargo_nextest", true)] {
        let mut group = group_with_driver("cargo");
        group.test_runner = runner.to_owned();
        let argv = task_argv(&group, &catalog).expect("task argv");
        assert_eq!(argv.contains(&nextest), want, "{runner} nextest");
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
        "mr-boxington@1.19.0",
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
