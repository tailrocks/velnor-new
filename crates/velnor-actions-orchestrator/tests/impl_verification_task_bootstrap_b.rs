use super::*;

#[test]
fn conflicting_task_tool_pins_and_nested_mise_invocations_fail_closed() {
    let root = tool_root();
    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[tasks.verify]
run = "echo verify"
depends = ["shared"]
tools = { "aqua:vendor/tool" = "2.3.4" }

[tasks.shared]
run = "echo shared"
tools = { "aqua:vendor/tool" = "1.0.0" }
"#,
    )
    .expect("write conflicting selected tool pins");
    let checks = check_tool_inputs(root.path());
    assert!(
        resolve_verification_tools(
            &checks,
            &verification_task("verify", VerificationRunner::LinuxX64)
        )
        .is_err()
    );

    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[tasks.verify]
run = "echo verify && mise run shared"

[tasks.shared]
run = "echo shared"
"#,
    )
    .expect("write unsupported nested invocation");
    let checks = check_tool_inputs(root.path());
    assert!(
        resolve_verification_tools(
            &checks,
            &verification_task("verify", VerificationRunner::LinuxX64)
        )
        .is_err()
    );
}

#[test]
fn cyclic_selected_task_dependencies_fail_closed() {
    let root = tool_root();
    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[tasks.verify]
run = "echo verify"
depends = ["shared"]

[tasks.shared]
run = "echo shared"
depends = ["verify"]
"#,
    )
    .expect("write task dependency cycle");
    let checks = check_tool_inputs(root.path());
    assert!(
        resolve_verification_tools(
            &checks,
            &verification_task("verify", VerificationRunner::LinuxX64)
        )
        .is_err()
    );
}

#[test]
fn empty_tool_closure_still_rejects_unsupported_settings_and_wrapper_root_keys() {
    let root = tool_root();
    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true
auto_install = true

[tasks.verify]
run = "echo verify"
"#,
    )
    .expect("write unsupported Mise setting");
    let checks = check_tool_inputs(root.path());
    let error = resolve_verification_tools(
        &checks,
        &verification_task("verify", VerificationRunner::LinuxX64),
    )
    .expect_err("unsupported settings fail closed without selected tools");
    assert!(error.to_string().contains("verification_mise_settings"));

    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true
idiomatic_version_file_enable_tools = "rust"

[tasks.verify]
run = "echo verify"
"#,
    )
    .expect("write malformed recognized setting");
    let checks = check_tool_inputs(root.path());
    let error = resolve_verification_tools(
        &checks,
        &verification_task("verify", VerificationRunner::LinuxX64),
    )
    .expect_err("malformed recognized settings fail closed");
    assert!(error.to_string().contains("verification_mise_settings"));

    for wrapper in [
        "command = \"cargo\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }",
        "command = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"0\" }",
    ] {
        let config = format!(
            "[settings]\nlockfile = true\n\n[tasks.verify]\nrun = \"echo verify\"\n\n[wrappers.cargo]\n{wrapper}\n"
        );
        fs::write(root.path().join("mise.toml"), config).expect("write unsupported wrapper");
        let checks = check_tool_inputs(root.path());
        let error = resolve_verification_tools(
            &checks,
            &verification_task("verify", VerificationRunner::LinuxX64),
        )
        .expect_err("unsupported wrapper fails closed without selected tools");
        assert!(error.to_string().contains("verification_mise_config_root"));
    }
}

#[test]
fn missing_root_mise_config_does_not_admit_unmodeled_task_files() {
    let root = tool_root();
    fs::remove_file(root.path().join("mise.toml")).expect("remove root task config");
    fs::create_dir(root.path().join(".mise")).expect("create task directory");
    fs::create_dir(root.path().join(".mise/tasks")).expect("create task directory");
    fs::write(
        root.path().join(".mise/tasks/verify"),
        "#!/bin/sh\nmise install cargo:unsafe-tool\n",
    )
    .expect("write unmodeled file task with install command");
    let checks = check_tool_inputs(root.path());
    let error = resolve_verification_tools(
        &checks,
        &verification_task("verify", VerificationRunner::LinuxX64),
    )
    .expect_err("unmodeled task files are not admitted without root mise.toml");
    assert!(
        error
            .to_string()
            .contains("verification_mise_config_missing")
    );
}

#[test]
fn commandless_task_metadata_cannot_overlay_any_default_task_file() {
    // The 2026-10-08 v2026.10.6 probe found file tasks under these default roots.
    // A metadata-only TOML task may overlay a same-named file task, retaining
    // its executable body, so every admitted node must provide its own run.
    for task_dir in [
        "mise-tasks",
        ".mise-tasks",
        ".mise/tasks",
        ".config/mise/tasks",
        "mise/tasks",
    ] {
        let root = tool_root();
        fs::write(
            root.path().join("mise.toml"),
            r#"
[settings]
lockfile = true

[tasks.verify]
depends = ["lint"]

[tasks.lint]
run = "echo lint"
"#,
        )
        .expect("write commandless selected task metadata");
        let task_file = root.path().join(task_dir).join("verify");
        fs::create_dir_all(task_file.parent().expect("task directory"))
            .expect("create task source directory");
        fs::write(&task_file, "#!/bin/sh\nmise install cargo:unsafe-tool\n")
            .expect("write unmodeled task body");

        let checks = check_tool_inputs(root.path());
        let error = resolve_verification_tools(
            &checks,
            &verification_task("verify", VerificationRunner::LinuxX64),
        )
        .expect_err("metadata-only task must not inherit a file task body");
        assert!(
            error
                .to_string()
                .contains("verification_task_inline_run_required"),
            "unexpected rejection for {task_dir}: {error}"
        );
    }
}

#[test]
fn nested_commandless_task_metadata_cannot_overlay_a_file_task() {
    let root = tool_root();
    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[tasks.verify]
run = "mise run aggregate"

[tasks.aggregate]
depends = ["lint"]

[tasks.lint]
run = "echo lint"
"#,
    )
    .expect("write task graph with commandless nested task");
    let task_file = root.path().join(".config/mise/tasks/aggregate");
    fs::create_dir_all(task_file.parent().expect("task directory"))
        .expect("create task source directory");
    fs::write(&task_file, "#!/bin/sh\nmise install cargo:unsafe-tool\n")
        .expect("write unmodeled nested task body");

    let checks = check_tool_inputs(root.path());
    let error = resolve_verification_tools(
        &checks,
        &verification_task("verify", VerificationRunner::LinuxX64),
    )
    .expect_err("nested metadata-only task must not inherit a file task body");
    assert!(
        error
            .to_string()
            .contains("verification_task_inline_run_required")
    );
}

#[test]
fn configured_task_directory_is_not_admitted() {
    let root = tool_root();
    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[task_config]
dir = "custom-tasks"

[tasks.verify]
depends = ["lint"]

[tasks.lint]
run = "echo lint"
"#,
    )
    .expect("write unsupported custom task directory");
    let task_file = root.path().join("custom-tasks/verify");
    fs::create_dir_all(task_file.parent().expect("task directory"))
        .expect("create custom task source directory");
    fs::write(&task_file, "#!/bin/sh\nmise install cargo:unsafe-tool\n")
        .expect("write unmodeled custom task body");

    let checks = check_tool_inputs(root.path());
    let error = resolve_verification_tools(
        &checks,
        &verification_task("verify", VerificationRunner::LinuxX64),
    )
    .expect_err("custom task directory config must fail closed");
    assert!(error.to_string().contains("verification_mise_config_root"));
}
