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
