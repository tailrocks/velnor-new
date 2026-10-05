//! Typed workflow-task routing through schema-2 execution modes.

use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

const SCALE_RUNS: &str = "runs-on: [velnor, ubuntu-26.04-scale-set, verification-worker]";

#[test]
fn verification_tasks_follow_eligible_lanes_and_stay_required() -> TestResult {
    for mode in ["hosted", "scale-set", "both"] {
        let repo = make_repo(&config(mode))?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
        let required = job_body(workflow, "required")?;
        let actionlint = job_body(workflow, "actionlint")?;
        let macos = job_body(workflow, "task-native-format")?;
        assert!(
            actionlint.contains("runs-on: ubuntu-26.04"),
            "{mode}: {actionlint}"
        );
        assert!(
            !workflow.contains("  actionlint__hosted:"),
            "{mode}: {workflow}"
        );
        assert!(
            !workflow.contains("  actionlint__local:"),
            "{mode}: {workflow}"
        );
        assert!(macos.contains("runs-on: macos-15"), "{mode}: {macos}");
        assert!(
            !workflow.contains("  task-native-format__hosted:"),
            "{mode}: {workflow}"
        );
        assert!(
            !workflow.contains("  task-native-format__local:"),
            "{mode}: {workflow}"
        );
        assert!(
            required.contains("- task-native-format"),
            "{mode}: {required}"
        );

        match mode {
            "hosted" => {
                let linux = job_body(workflow, "task-linux-lint")?;
                assert!(linux.contains("runs-on: ubuntu-26.04"), "{linux}");
                assert!(required.contains("- task-linux-lint"), "{required}");
                assert!(!workflow.contains("  task-linux-lint__hosted:"));
                assert!(!workflow.contains("  task-linux-lint__local:"));
            }
            "scale-set" => {
                let linux = job_body(workflow, "task-linux-lint")?;
                assert!(linux.contains(SCALE_RUNS), "{linux}");
                assert!(required.contains("- task-linux-lint"), "{required}");
                assert!(!workflow.contains("  task-linux-lint__hosted:"));
                assert!(!workflow.contains("  task-linux-lint__local:"));
            }
            "both" => {
                let hosted = job_body(workflow, "task-linux-lint__hosted")?;
                let scale = job_body(workflow, "task-linux-lint__local")?;
                assert!(hosted.contains("runs-on: ubuntu-26.04"), "{hosted}");
                assert!(scale.contains(SCALE_RUNS), "{scale}");
                assert!(required.contains("- task-linux-lint__hosted"), "{required}");
                assert!(required.contains("- task-linux-lint__local"), "{required}");
                assert!(!workflow.contains("  task-linux-lint:"));
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

#[test]
fn macos_task_cannot_select_the_linux_scale_set_profile() -> TestResult {
    let config = format!(
        "{}\n[execution.overrides.\"task-native-format\"]\nprofile = \"local\"\nrole = \"verification\"",
        config("hosted")
    );
    let repo = make_repo(&config)?;
    let prep = prepare(repo.path())?;
    let error = render_staged_tree(&prep).expect_err("macOS cannot target Linux Scale Set");
    assert!(
        error
            .to_string()
            .contains("verification_runner_incompatible_with_scale_set"),
        "{error}"
    );
    Ok(())
}

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
        assert_selected_mise_projection(&preparation);
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
) {
    let job = preparation
        .workflow
        .ir
        .jobs
        .get("task-native-desktop")
        .expect("native build task job");
    let StepKind::Shell { run, .. } = &job.steps[3].kind else {
        panic!("selected-tool bootstrap is not a shell step");
    };
    let script = run.last().expect("shell script argument");
    let config_text = heredoc_body(script, "VELNOR_SELECTED_MISE_CONFIG");
    let config = toml::from_str::<toml::Value>(config_text).expect("generated Mise config parses");
    let expected = toml::from_str::<toml::Value>(EXPECTED_SELECTED_MISE_CONFIG)
        .expect("expected Mise config parses");
    assert_eq!(
        config, expected,
        "selected tool selectors stay in their own tables"
    );

    let lock_text = heredoc_body(script, "VELNOR_SELECTED_MISE_LOCK");
    let lock = toml::from_str::<toml::Value>(lock_text).expect("generated Mise lock parses");
    let source_rust_options = lock["tools"]["rust"][0]["options"]
        .as_table()
        .expect("source-bound locked Rust options");
    let installed_rust = config["tools"]["rust"]
        .as_table()
        .expect("Rust selector table");
    assert_eq!(
        source_rust_options.get("components"),
        installed_rust.get("components")
    );
    assert_eq!(
        source_rust_options.get("targets"),
        installed_rust.get("targets")
    );
}

fn heredoc_body<'a>(script: &'a str, delimiter: &str) -> &'a str {
    let marker = format!("<<'{delimiter}'\n");
    let (_, remainder) = script
        .split_once(&marker)
        .expect("generated heredoc marker");
    let end = format!("\n{delimiter}");
    remainder
        .split_once(&end)
        .expect("generated heredoc terminator")
        .0
}

const EXPECTED_SELECTED_MISE_CONFIG: &str = r#"
[tools."aqua:nextest-rs/nextest/cargo-nextest"]
version = "0.9.140"
[tools."github:boltffi/boltffi"]
version = "0.30.1"
matching_regex = '^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$'
[tools."mr-boxington"]
version = "1.22.0"
[tools."rust"]
version = "1.97.1"
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
[wrappers.cargo]
command = "mbx"
[wrappers.cargo.env]
MBX_CARGO_SHIM_MODE = "1"
"#;

#[test]
fn native_build_task_rejects_cargo_source_install_fallback() -> TestResult {
    let body = config_with_build("hosted");
    let repo = make_repo(&body)?;
    write_native_source_fixture(repo.path())?;
    let lock_path = repo.path().join("mise.lock");
    let lock = fs::read_to_string(&lock_path)?.replace(
        "backend = \"github:boltffi/boltffi\"",
        "backend = \"cargo:boltffi_cli\"",
    );
    fs::write(lock_path, lock)?;

    let prep = prepare(repo.path())?;
    let error = render_staged_tree(&prep).expect_err("Cargo fallback backend must fail closed");
    assert!(
        error
            .to_string()
            .contains("build_task_tool_backend_unsupported"),
        "{error}"
    );
    Ok(())
}

/// Synthetic source-bound task/tool fixture for the resolver integration.
/// Artifact digests and URLs test the lock projection shape only; upstream
/// provenance is separately reviewed against the repository's real Mise lock.
fn write_native_source_fixture(root: &std::path::Path) -> TestResult {
    fs::write(
        root.join("mise.toml"),
        r#"
[tools]
"aqua:nextest-rs/nextest/cargo-nextest" = "0.9.140"
"github:boltffi/boltffi" = { version = "0.30.1", matching_regex = '^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$' }
"mr-boxington" = "1.22.0"
swiftlint = { version = "0.65.1", os = ["macos"] }
xcodegen = "2.46.0"

[settings]
lockfile = true
idiomatic_version_file_enable_tools = ["rust"]

[settings.cargo]
binstall = true

[wrappers.cargo]
command = "mbx"

[wrappers.cargo.env]
MBX_CARGO_SHIM_MODE = "1"

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
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.97.1\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n",
    )?;
    fs::write(
        root.join("mise.lock"),
        r#"
[tools]
"aqua:nextest-rs/nextest/cargo-nextest" = [{ version = "0.9.140", backend = "aqua:nextest-rs/nextest/cargo-nextest", "platforms.macos-arm64" = { url = "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.140/cargo-nextest-0.9.140-universal-apple-darwin.tar.gz", checksum = "sha256:58e0a722f9444078fab447783f322acf15a2a771ba785b3fbbe8bacda31c3df9" } }]
"github:boltffi/boltffi" = [{ version = "0.30.1", backend = "github:boltffi/boltffi", options = { matching_regex = '^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$' }, "platforms.macos-arm64" = { url = "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz", checksum = "sha256:ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a" } }]
"mr-boxington" = [{ version = "1.22.0", backend = "packslip:github.com/jdx/mr-boxington", "platforms.macos-arm64" = { url = "https://github.com/jdx/mr-boxington/releases/download/v1.22.0/mbx-aarch64-apple-darwin.tar.gz", checksum = "sha256:e548b5758498cf822a180b6328597e6aded8fe9bb3046cd918399172ae30dde2", signer = "sigstore-oidc:https://github.com/jdx/mr-boxington/.github/workflows/release.yml" } }]
rust = [{ version = "1.97.1", backend = "core:rust", options = { components = "clippy,rustfmt", targets = "aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu" } }]
swiftlint = [{ version = "0.65.1", backend = "aqua:realm/SwiftLint", "platforms.macos-arm64" = { url = "https://github.com/realm/SwiftLint/releases/download/0.65.1/portable_swiftlint.zip", checksum = "sha256:c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0" } }]
xcodegen = [{ version = "2.46.0", backend = "aqua:yonaskolb/XcodeGen", "platforms.macos-arm64" = { url = "https://github.com/yonaskolb/XcodeGen/releases/download/2.46.0/xcodegen.zip", checksum = "sha256:4d9e34b62172d645eed6457cac13fc222569974098ef4ee9c3368bedf0196806" } }]
"#,
    )?;
    Ok(())
}

fn config(mode: &str) -> String {
    format!(
        "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[workflow.tasks]]\nid = \"linux-lint\"\nkind = \"verification\"\nmise_task = \"lint-linux\"\nrunner = \"linux-x64\"\ntimeout_minutes = 10\n\
[[workflow.tasks]]\nid = \"native-format\"\nkind = \"verification\"\nmise_task = \"desktop-format-check\"\nrunner = \"macos-arm64\"\ntimeout_minutes = 10\n\
[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"{mode}\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\", \"verification-worker\"]\nplatform = \"linux/amd64\""
    )
}

fn config_with_build(mode: &str) -> String {
    let build = "[[workflow.tasks]]\nid = \"native-desktop\"\nkind = \"build\"\nmise_task = \"desktop-ci\"\ntools = [\"aqua:nextest-rs/nextest/cargo-nextest\", \"github:boltffi/boltffi\", \"mr-boxington\", \"rust\", \"swiftlint\", \"xcodegen\"]\nrunner = \"macos-26-arm64\"\ntimeout_minutes = 120\ncargo_build_jobs = 2\nnextest_test_threads = 2\n";
    config(mode).replace(
        "[[workflow.tasks]]\nid = \"native-format\"",
        &format!("{build}[[workflow.tasks]]\nid = \"native-format\""),
    )
}
