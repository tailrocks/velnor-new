//! Verification tool bootstrap admits only the selected task's locked closure.

use std::fs;

use velnor_actions_contract::VerificationRunner;

use super::resolve_verification_tools;
use crate::toolcheck::check_tool_inputs;

fn tool_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temporary tool input root");
    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[tools."cargo:codebook-lsp"]
version = "0.1.0"

[tasks.verify]
run = "mise run lint"
depends = ["shared"]

[tasks.shared]
run = "echo shared"
tools = { "aqua:vendor/tool" = "2.3.4" }

[tasks.lint]
run = "echo lint"
tools = { "github:example/linter" = "1.2.3" }
"#,
    )
    .expect("write selected task config");
    fs::write(
        root.path().join("mise.lock"),
        r#"
[tools]
"aqua:vendor/tool" = [
  { version = "2.3.4", backend = "aqua:vendor/tool", "platforms.linux-x64" = { url = "https://github.com/vendor/tool/releases/download/v2.3.4/tool-linux-x64.tar.gz", checksum = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }, "platforms.macos-arm64" = { url = "https://github.com/vendor/tool/releases/download/v2.3.4/tool-darwin-arm64.tar.gz", checksum = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" } }
]
"github:example/linter" = [
  { version = "1.2.3", backend = "github:example/linter", "platforms.linux-x64" = { url = "https://github.com/example/linter/releases/download/v1.2.3/linter-linux-x64.tar.gz", checksum = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" }, "platforms.macos-arm64" = { url = "https://github.com/example/linter/releases/download/v1.2.3/linter-darwin-arm64.tar.gz", checksum = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd" } }
]
"cargo:codebook-lsp" = [
  { version = "0.1.0", backend = "cargo:codebook-lsp" }
]
"#,
    )
    .expect("write project lock with unrelated Cargo entry");
    root
}

#[test]
fn only_transitive_task_tools_are_selected_and_platform_rows_are_bound() {
    let root = tool_root();
    let checks = check_tool_inputs(root.path());
    let linux = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
        .expect("locked Linux task tool closure");
    let macos = resolve_verification_tools(&checks, "verify", VerificationRunner::MacosArm64)
        .expect("locked macOS task tool closure");

    assert_eq!(linux.selected_tools.len(), 2);
    assert_eq!(
        linux
            .selected_tools
            .iter()
            .map(|tool| tool.key.as_str())
            .collect::<Vec<_>>(),
        ["aqua:vendor/tool", "github:example/linter"]
    );
    assert!(
        linux
            .selected_tools
            .iter()
            .all(|tool| tool.key != "cargo:codebook-lsp")
    );
    assert_eq!(
        linux.selected_tools[0]
            .artifact
            .as_ref()
            .expect("Linux lock artifact")
            .url,
        "https://github.com/vendor/tool/releases/download/v2.3.4/tool-linux-x64.tar.gz"
    );
    assert_eq!(
        macos.selected_tools[0]
            .artifact
            .as_ref()
            .expect("macOS lock artifact")
            .url,
        "https://github.com/vendor/tool/releases/download/v2.3.4/tool-darwin-arm64.tar.gz"
    );
    assert!(linux.mise_config_sha256.is_some());
    assert!(linux.mise_lock_sha256.is_some());
}

#[test]
fn selected_cargo_tools_and_missing_locked_rows_fail_closed() {
    let root = tool_root();
    let config = r#"
[settings]
lockfile = true

[tasks.verify]
run = "echo verify"
tools = { "cargo:unsafe-tool" = "1.2.3" }
"#;
    fs::write(root.path().join("mise.toml"), config).expect("replace config fixture");
    fs::write(
        root.path().join("mise.lock"),
        r#"
[tools]
"cargo:unsafe-tool" = [
  { version = "1.2.3", backend = "cargo:unsafe-tool" }
]
"#,
    )
    .expect("write Cargo source lock fixture");
    let checks = check_tool_inputs(root.path());
    assert!(resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64).is_err());

    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[tasks.verify]
run = "echo verify"
tools = { "github:example/linter" = "1.2.3" }
"#,
    )
    .expect("write missing-lock-row config");
    fs::write(root.path().join("mise.lock"), "[tools]\n").expect("remove selected lock row");
    let checks = check_tool_inputs(root.path());
    assert!(resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64).is_err());
}

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
    assert!(resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64).is_err());

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
    assert!(resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64).is_err());
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
    assert!(resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64).is_err());
}

#[test]
fn empty_tool_closure_still_rejects_unsupported_settings_and_wrappers() {
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
    let error = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
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
    let error = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
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
        let error = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
            .expect_err("unsupported wrapper fails closed without selected tools");
        assert!(error.to_string().contains("verification_mise_wrappers"));
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
    let error = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
        .expect_err("unmodeled task files are not admitted without root mise.toml");
    assert!(
        error
            .to_string()
            .contains("verification_mise_config_missing")
    );
}

#[test]
fn commandless_task_metadata_cannot_overlay_any_default_task_file() {
    // Mise v2026.10.4 discovers file tasks from each of these default roots.
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
        let error = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
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
    let error = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
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
    let error = resolve_verification_tools(&checks, "verify", VerificationRunner::LinuxX64)
        .expect_err("custom task directory config must fail closed");
    assert!(error.to_string().contains("verification_mise_config_root"));
}
