use super::*;

#[test]
fn env_source_overlay_is_suppressed_before_config_inspection_and_task_execution() {
    let overlay_fixture = "[env]\n_.source = \"./sentinel.sh\"\n";
    assert!(overlay_fixture.contains("_.source"));

    let policy = policy();
    let bootstrap = install_selected_tools_script(&policy).expect("bootstrap script");
    let env_disable = bootstrap
        .find("export MISE_NO_ENV=1")
        .expect("no-env export");
    let config_inspection = bootstrap
        .find("mise --no-env --no-hooks config ls --json")
        .expect("no-env config inspection");
    assert!(env_disable < config_inspection, "{bootstrap}");

    let run = run_build_task_script(&policy).expect("run script");
    let env_disable = run.find("export MISE_NO_ENV=1").expect("no-env export");
    let task = run
        .find("mise --no-env --locked --no-hooks run --skip-tools desktop-ci")
        .expect("no-env task run");
    assert!(env_disable < task, "{run}");
    assert!(run.contains("unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE"));
}

#[test]
fn nested_task_source_is_hash_bound_and_mbx_preserves_its_working_directory() {
    let policy = policy();
    let script = source_guard_script(&policy).expect("source guard");
    let expected = [
        "test -f \"$workspace_root/mise.toml\"",
        "test -f \"$workspace_root/native/mise.toml\"",
        "$workspace_root/native/mise.toml",
        "test ! -L \"$workspace_root/native\"",
        "cd -P \"$workspace_root/native\"",
        "task_working_directory=\"$PWD\"",
        "export MISE_CEILING_PATHS=\"$workspace_ceiling\"",
        "workspace_ceiling=\"$workspace_root/..\"",
        "\"$mbx_path\" exec --project-root \"$workspace_root\" \"$rustc_path\" --version",
        "cd -P \"$task_working_directory\"",
    ];
    for fragment in expected {
        assert!(script.contains(fragment), "missing {fragment}: {script}");
    }
    assert!(script.contains("$workspace_root/rust-toolchain.toml"));
    let enter = script
        .find("cd -P \"$workspace_root/native\"")
        .expect("declared source working directory");
    let restore = script
        .find("cd -P \"$task_working_directory\"")
        .expect("MBX guard restores declared directory");
    assert!(enter < restore, "{script}");
}

#[test]
fn selected_config_and_lock_preserve_only_selected_platform_pins() {
    let (config, lock) = selected_mise_files(&policy()).expect("selected source projection");
    assert!(config.contains("[tools.\"mr-boxington\"]\nversion = \"1.23.0\""));
    assert!(config.contains("[tools.\"rust\"]\nversion = \"1.99.0\""));
    assert!(config.contains("components = \"clippy,rustfmt\""));
    assert!(config.contains("targets = \"aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu\""));
    assert!(config.contains("\"github:boltffi/boltffi\""));
    assert!(config.contains(BOLTFFI_MATCHING_REGEX));
    assert!(config.contains("os = [\"macos\"]"));
    assert!(!config.contains("wrappers"));
    assert!(!config.contains("command = \"mbx\""));
    assert!(lock.contains("backend = \"packslip:github.com/jdx/mr-boxington\""));
    assert!(lock.contains("sha256:"));
    assert!(lock.contains("platforms.macos-arm64"));
    assert!(lock.contains("boltffi-darwin-aarch64.tar.gz"));
    assert!(!config.contains("codebook-lsp"));
    assert!(!lock.contains("cargo:boltffi_cli"));
}

#[test]
fn unsupported_source_backend_and_unbound_artifacts_fail_closed() {
    let mut invalid_policy = policy();
    let cargo = BuildTaskTool {
        key: "cargo:unsafe-tool".to_owned(),
        version: "1.0.0".to_owned(),
        backend: "cargo:unsafe-tool".to_owned(),
        os: Vec::new(),
        config_options: BTreeMap::new(),
        lock_options: BTreeMap::new(),
        artifact: Some(artifact(
            "https://github.com/example/tool/releases/download/v1.0/tool-macos-arm64.tar.gz",
            &"e".repeat(64),
        )),
    };
    invalid_policy.task.tools = vec![
        "cargo:unsafe-tool".to_owned(),
        "mr-boxington".to_owned(),
        "rust".to_owned(),
    ];
    let mbx = invalid_policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == "mr-boxington")
        .cloned()
        .expect("MBX tool");
    let rust = invalid_policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == "rust")
        .cloned()
        .expect("Rust tool");
    invalid_policy.selected_tools = vec![cargo, mbx, rust];
    assert!(selected_mise_files(&invalid_policy).is_err());

    let mut bad_asset_policy = policy();
    bad_asset_policy.selected_tools[0]
        .artifact
        .as_mut()
        .expect("asset")
        .url = "https://evil.example/tool.tar.gz".to_owned();
    assert!(install_selected_tools_script(&bad_asset_policy).is_err());
    assert!(build_build_task_job(&bad_asset_policy, CHECKOUT).is_err());
}

#[test]
fn selected_mbx_or_rust_pin_tampering_is_rejected() {
    for (key, version) in [("mr-boxington", "1.22.0"), ("rust", "1.97.1")] {
        let mut changed = policy();
        changed
            .selected_tools
            .iter_mut()
            .find(|tool| tool.key == key)
            .expect("selected tool")
            .version = version.to_owned();
        assert!(
            build_build_task_job(&changed, CHECKOUT).is_err(),
            "tampered {key} pin must not produce executable authority"
        );
    }
}

#[test]
fn declared_jobs_are_exact_sorted_and_unique() {
    let policy = policy();
    let id = policy.job_id();
    let job = build_build_task_job(&policy, CHECKOUT).expect("native task job");
    let jobs = BTreeMap::from([(id.clone(), job.clone())]);
    assert_eq!(
        validate_build_task_jobs(&jobs, std::slice::from_ref(&policy), CHECKOUT)
            .expect("declared native job")
            .as_slice(),
        std::slice::from_ref(&id)
    );

    let mut changed = jobs;
    changed.insert(
        id,
        Job {
            timeout_minutes: velnor_actions_contract::JobTimeout::new(119).expect("timeout"),
            ..job
        },
    );
    assert!(validate_build_task_jobs(&changed, &[policy], CHECKOUT).is_err());
}

#[test]
fn emitted_scripts_are_single_line_without_command_substitution() {
    let job = build_build_task_job(&policy(), CHECKOUT).expect("native task job");
    for step in &job.steps {
        let StepKind::Shell { run, .. } = &step.kind else {
            continue;
        };
        for arg in run {
            assert!(!arg.contains('\n'), "single-line script: {arg}");
            assert!(!arg.contains("$("), "no substitution: {arg}");
            assert!(!arg.contains('`'), "no backticks: {arg}");
        }
    }
    let bootstrap = install_selected_tools_script(&policy()).expect("bootstrap script");
    assert!(bootstrap.contains("printf '%s\\n'"));
    assert!(!bootstrap.contains("<<'VELNOR_SELECTED_MISE_CONFIG'"));
}
