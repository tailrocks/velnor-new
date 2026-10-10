//! Native build-task routing through schema-2 execution modes.

use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};
use crate::impl_schema2_verification_tasks::config;

#[test]
fn native_build_variant_rejects_scale_set_profile_override() -> TestResult {
    let body = format!(
        "{}\n[execution.overrides.\"task-native-desktop\"]\nprofile = \"local\"\nrole = \"verification\"",
        config_with_build("hosted")
    );
    let repo = make_repo(&body)?;
    write_native_source_fixture(repo.path())?;
    let preparation = prepare(repo.path())?;
    let error = render_staged_tree(&preparation).expect_err("native build task is hosted-only");
    assert!(
        error
            .to_string()
            .contains("workflow_task_hosted_only_scale_set_override"),
        "{error}"
    );
    Ok(())
}

#[test]
fn native_build_variant_is_shared_source_bounded_and_required_in_every_mode() -> TestResult {
    for mode in ["hosted", "scale-set", "both"] {
        let body = config_with_build(mode);
        let repo = make_repo(&body)?;
        write_native_source_fixture(repo.path())?;
        let preparation = prepare(repo.path())?;
        assert_selected_mise_projection(&preparation)?;
        let tree = render_staged_tree(&preparation)?;
        let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
        let required = job_body(workflow, "required")?;
        let native = job_body(workflow, "task-native-desktop")?;

        assert!(native.contains("runs-on: macos-26"), "{mode}: {native}");
        assert!(native.contains("timeout-minutes: 120"), "{mode}: {native}");
        assert!(
            native.contains("CARGO_BUILD_JOBS: \"2\""),
            "{mode}: {native}"
        );
        assert!(
            native.contains("NEXTEST_TEST_THREADS: \"2\""),
            "{mode}: {native}"
        );
        assert!(
            native.contains("MISE_CARGO_BINSTALL_ONLY: \"1\""),
            "{mode}: {native}"
        );
        assert!(native.contains("DEVELOPER_DIR:"), "{mode}: {native}");
        assert!(
            native.contains("persist-credentials: \"false\""),
            "{mode}: {native}"
        );
        assert!(
            native.contains("wrappers.cargo.command"),
            "{mode}: {native}"
        );
        assert!(
            native.contains("mise --no-env --locked --no-hooks run --skip-tools desktop-ci"),
            "{mode}: {native}"
        );
        assert!(
            native.contains(r#"cd -P \"$workspace_root/native\"; task_working_directory=\"$PWD\""#),
            "{mode}: task must run from its declared nested directory: {native}"
        );
        assert!(
            native.contains("$workspace_root/native/mise.toml")
                && native.contains("$workspace_root/mise.toml"),
            "{mode}: source task config and root tool config are hash-bound: {native}"
        );
        assert!(
            native.contains(r#"export MISE_CEILING_PATHS=\"$workspace_ceiling\""#)
                && native.contains(r#"workspace_ceiling=\"$workspace_root/..\""#),
            "{mode}: config discovery includes the root and nested source: {native}"
        );
        let restore_cwd = native
            .find(r#"cd -P \"$task_working_directory\""#)
            .ok_or("MBX guard restores the task cwd")?;
        let run_task = native
            .find("mise --no-env --locked --no-hooks run --skip-tools desktop-ci")
            .ok_or("declared task is executed")?;
        assert!(restore_cwd < run_task, "{mode}: {native}");
        let nested = fs::read_to_string(repo.path().join("native/mise.toml"))?;
        assert!(nested.contains("run = \"mise run desktop-lint\""));
        let root_mise = fs::read_to_string(repo.path().join("mise.toml"))?;
        assert!(root_mise.contains("description = \"Native desktop build task\""));
        assert!(native.contains("export MISE_NO_ENV=1"), "{mode}: {native}");
        assert!(
            native.contains("github:boltffi/boltffi")
                && native.contains("aqua:nextest-rs/nextest/cargo-nextest")
                && native.contains("swiftlint")
                && native.contains("xcodegen"),
            "{mode}: selected native tool closure is incomplete: {native}"
        );
        assert!(!native.contains("repository:"), "{mode}: {native}");
        assert!(!native.contains("ref:"), "{mode}: {native}");
        assert!(!native.contains("actions/cache@"), "{mode}: {native}");
        assert!(
            !native.contains("actions/upload-artifact@"),
            "{mode}: {native}"
        );
        assert!(
            required.contains("- task-native-desktop"),
            "{mode}: {required}"
        );
        assert!(required.contains("VELNOR_NEEDS_JSON"), "{mode}: {required}");
        assert!(
            required.contains("VELNOR_NEEDS_EXPECTED"),
            "{mode}: {required}"
        );
        assert!(
            required.lines().any(|line| {
                line.contains("VELNOR_NEEDS_EXPECTED") && line.contains("task-native-desktop")
            }),
            "{mode}: the required conclusion inventory must include the native job: {required}"
        );
        assert!(!workflow.contains("task-native-desktop__hosted"));
        assert!(!workflow.contains("task-native-desktop__local"));
    }
    Ok(())
}

fn assert_selected_mise_projection(
    preparation: &velnor_actions_orchestrator::GenerationPreparation,
) -> TestResult {
    let job = preparation
        .workflow
        .ir
        .jobs
        .get("task-native-desktop")
        .ok_or("native build task job")?;
    let StepKind::Shell { run, .. } = &job.steps[3].kind else {
        return Err("selected-tool bootstrap is not a shell step".into());
    };
    let script = run.last().ok_or("shell script argument")?;
    let config_text = printf_body(script, "$task_root/mise.toml")?;
    let config = toml::from_str::<toml::Value>(&config_text)?;
    let expected = toml::from_str::<toml::Value>(EXPECTED_SELECTED_MISE_CONFIG)?;
    assert_eq!(
        config, expected,
        "selected tool selectors stay in their own tables"
    );

    let lock_text = printf_body(script, "$task_root/mise.lock")?;
    let lock = toml::from_str::<toml::Value>(&lock_text)?;
    let source_rust_options = lock["tools"]["rust"][0]["options"]
        .as_table()
        .ok_or("source-bound locked Rust options")?;
    let installed_rust = config["tools"]["rust"]
        .as_table()
        .ok_or("Rust selector table")?;
    assert_eq!(
        source_rust_options.get("components"),
        installed_rust.get("components")
    );
    assert_eq!(
        source_rust_options.get("targets"),
        installed_rust.get("targets")
    );
    Ok(())
}

/// Reconstruct one `printf`-written file from the bootstrap script.
///
/// Parses the single-quoted literal and escaped double-quoted data
/// arguments exactly as POSIX shell would split them.
fn printf_body(script: &str, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let redirect = format!(" > \"{path}\"");
    let (head, _) = script
        .split_once(&redirect)
        .ok_or("generated printf redirect")?;
    let args = head
        .rsplit_once("printf '%s\\n' ")
        .ok_or("generated printf command")?
        .1;
    Ok(parse_printf_args(args)?.join("\n"))
}

fn parse_printf_args(args: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    let mut chars = args.chars();
    loop {
        match chars.next() {
            None => return Ok(out),
            Some(' ') => {}
            Some('\'') => {
                let mut line = String::new();
                for char in chars.by_ref() {
                    if char == '\'' {
                        break;
                    }
                    line.push(char);
                }
                out.push(line);
            }
            Some('"') => {
                let mut line = String::new();
                let mut escaped = false;
                for char in chars.by_ref() {
                    if escaped {
                        if !matches!(char, '\\' | '"' | '$' | '`') {
                            return Err(format!("unexpected printf escape: {char}").into());
                        }
                        line.push(char);
                        escaped = false;
                    } else if char == '\\' {
                        escaped = true;
                    } else if char == '"' {
                        break;
                    } else {
                        line.push(char);
                    }
                }
                if escaped {
                    return Err("dangling printf escape".into());
                }
                out.push(line);
            }
            Some(other) => return Err(format!("unexpected printf argument start: {other}").into()),
        }
    }
}

const EXPECTED_SELECTED_MISE_CONFIG: &str = r#"
[tools."aqua:nextest-rs/nextest/cargo-nextest"]
version = "0.9.140"
[tools."github:boltffi/boltffi"]
version = "0.30.1"
matching_regex = '^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$'
[tools."mr-boxington"]
version = "1.23.0"
[tools."rust"]
version = "1.99.0"
mr_boxington = true
components = "clippy,rustfmt"
targets = "aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu"
[tools."swiftlint"]
version = "0.65.1"
os = ["macos"]
[tools."xcodegen"]
version = "2.46.0"
[settings]
lockfile = true
[settings.cargo]
binstall = true
binstall_only = true
"#;

/// Synthetic source-bound task/tool fixture for the resolver integration.
/// Artifact digests and URLs test the lock projection shape only; upstream
/// provenance is separately reviewed against the repository's real Mise lock.
pub(crate) fn write_native_source_fixture(root: &std::path::Path) -> TestResult {
    fs::write(
        root.join("mise.toml"),
        r#"
min_version = "2026.10.7"

[tools]
"aqua:nextest-rs/nextest/cargo-nextest" = "0.9.140"
"github:boltffi/boltffi" = { version = "0.30.1", matching_regex = '^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$' }
"mr-boxington" = "1.23.0"
rust = { version = "1.99.0", mr_boxington = true }
swiftlint = { version = "0.65.1", os = ["macos"] }
xcodegen = "2.46.0"

[settings]
lockfile = true
idiomatic_version_file_enable_tools = ["rust"]

[settings.cargo]
binstall = true

[tasks.lint-linux]
run = "echo lint-linux"

[tasks.desktop-ci]
description = "Native desktop build task"
run = '''
set -euo pipefail
mise run desktop-bindings-check
mise run desktop-generate
mise run desktop-format-check
mise run desktop-lint
mise run desktop-test
mise run desktop-build
cargo xtask desktop test-swift --jobs 2
mise run desktop-verify
'''

[tasks.desktop-bindings-check]
run = "cargo xtask desktop bindings-check"

[tasks.desktop-generate]
run = "xcodegen generate --spec native/project.yml"

[tasks.desktop-format-check]
run = "cd native && swift-format lint --strict"

[tasks.desktop-lint]
run = "cd native && swiftlint lint --strict"

[tasks.desktop-test]
run = "cargo xtask desktop test"

[tasks.desktop-build]
usage = 'arg "[version]"'
run = "cargo xtask desktop build"

[tasks.desktop-verify]
usage = 'arg "<app>"'
run = "cargo xtask desktop verify"
"#,
    )?;
    fs::create_dir_all(root.join("native"))?;
    fs::write(
        root.join("native/mise.toml"),
        "[tasks.desktop-ci]\nrun = \"mise run desktop-lint\"\n\n[tasks.format-check]\nrun = \"echo format-check\"\n",
    )?;
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.99.0\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n",
    )?;
    fs::write(
        root.join("mise.lock"),
        r#"
lockfile_version = 3

[tools]
"aqua:nextest-rs/nextest/cargo-nextest" = [{ version = "0.9.140", backend = "aqua:nextest-rs/nextest/cargo-nextest", specifiers = ["0.9.140"], "platforms.macos-arm64" = { url = "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.140/cargo-nextest-0.9.140-universal-apple-darwin.tar.gz", checksum = "sha256:58e0a722f9444078fab447783f322acf15a2a771ba785b3fbbe8bacda31c3df9" } }]
"github:boltffi/boltffi" = [{ version = "0.30.1", backend = "github:boltffi/boltffi", specifiers = ["0.30.1"], options = { matching_regex = '^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$' }, "platforms.macos-arm64" = { url = "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz", checksum = "sha256:ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a" } }]
"mr-boxington" = [{ version = "1.23.0", backend = "packslip:github.com/jdx/mr-boxington", specifiers = ["1.23.0"], "platforms.macos-arm64" = { url = "https://github.com/jdx/mr-boxington/releases/download/v1.23.0/mbx-aarch64-apple-darwin.tar.gz", checksum = "sha256:e548b5758498cf822a180b6328597e6aded8fe9bb3046cd918399172ae30dde2", signer = "sigstore-oidc:https://github.com/jdx/mr-boxington/.github/workflows/release.yml" } }]
rust = [{ version = "1.99.0", backend = "core:rust", specifiers = ["1.99.0"], options = { components = "clippy,rustfmt", targets = "aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu" } }]
swiftlint = [{ version = "0.65.1", backend = "aqua:realm/SwiftLint", specifiers = ["0.65.1"], "platforms.macos-arm64" = { url = "https://github.com/realm/SwiftLint/releases/download/0.65.1/portable_swiftlint.zip", checksum = "sha256:c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0" } }]
xcodegen = [{ version = "2.46.0", backend = "aqua:yonaskolb/XcodeGen", specifiers = ["2.46.0"], "platforms.macos-arm64" = { url = "https://github.com/yonaskolb/XcodeGen/releases/download/2.46.0/xcodegen.zip", checksum = "sha256:4d9e34b62172d645eed6457cac13fc222569974098ef4ee9c3368bedf0196806" } }]
"#,
    )?;
    Ok(())
}

pub(crate) fn config_with_build(mode: &str) -> String {
    let build = "[[workflow.tasks]]\nid = \"native-desktop\"\nkind = \"build\"\nmise_task = \"desktop-ci\"\nsource = { mise_config = \"native/mise.toml\", working_directory = \"native\" }\ntools = [\"aqua:nextest-rs/nextest/cargo-nextest\", \"github:boltffi/boltffi\", \"mr-boxington\", \"rust\", \"swiftlint\", \"xcodegen\"]\nrunner = \"macos-26-arm64\"\ntimeout_minutes = 120\ncargo_build_jobs = 2\nnextest_test_threads = 2\n";
    config(mode).replace(
        "[[workflow.tasks]]\nid = \"native-format\"",
        &format!("{build}[[workflow.tasks]]\nid = \"native-format\""),
    )
}
