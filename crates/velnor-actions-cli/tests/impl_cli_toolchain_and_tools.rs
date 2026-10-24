//! End-to-end CLI tests for repository Rust toolchain resolution (`rust-toolchain.toml`,
//! `rust-toolchain`, `[stacks.rust] toolchain`, `[tools] rust`), `RUSTUP_TOOLCHAIN`
//! omission when a repository toolchain file is present, and `[tools]` version overrides.

use std::error::Error;
use std::fs;
use std::path::Path;

use crate::impl_cli_tmp::{
    cleanup, code, commit_all, fresh_tempdir, git_init, install_consumer_manifest, spawn,
    write_workspace,
};

#[cfg(unix)]
use crate::impl_cli_tmp::{MiseDiagnosticContext, mise_failure_diagnostics, tool_context};

#[cfg(unix)]
#[path = "impl_cli_toolchain_fixture.rs"]
mod toolchain_fixture;

const NEXTEST_CONFIG: &str = r"[profile.ci]
fail-fast = false

[profile.smoke]
fail-fast = false
";

fn setup_repo(repo: &Path, config_toml: &str) -> Result<(), Box<dyn Error>> {
    git_init(repo)?;
    write_workspace(repo, 1)?;
    fs::create_dir_all(repo.join(".velnor"))?;
    fs::write(repo.join(".velnor/config.toml"), config_toml)?;
    install_consumer_manifest(repo)?;
    fs::create_dir_all(repo.join(".config"))?;
    fs::write(repo.join(".config/nextest.toml"), NEXTEST_CONFIG)?;
    Ok(())
}

fn config_with_suites(extra_rust: &str, extra_root: &str) -> String {
    format!(
        r#"schema = 1

[workflow]
name = "CI"
default_branch = "main"

[stacks.rust]
test_runner = "cargo_nextest"
{extra_rust}
[[stacks.rust.suites]]
id = "smoke"
profile = "smoke"
triggers = ["pull_request", "push", "workflow_dispatch"]
{extra_root}"#
    )
}

/// Whether mise reports `version` installed for rust on this host.
///
/// Inventory executes the repo-resolved pin and fails closed when it is
/// absent, so these tests branch on presence instead of assuming the
/// compile-time pin.
fn mise_has_rust(version: &str) -> bool {
    let Ok(output) = std::process::Command::new("mise")
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "ls",
            "--json",
            "rust",
        ])
        .output()
    else {
        return false;
    };
    let Ok(text) = String::from_utf8(output.stdout) else {
        return false;
    };
    let Ok(entries) = serde_json::from_str::<serde_json::Value>(&text).map(|value| match value {
        serde_json::Value::Array(entries) => entries,
        _ => Vec::new(),
    }) else {
        return false;
    };
    entries.iter().any(|entry| {
        entry.get("version").and_then(|seen| seen.as_str()) == Some(version)
            && entry.get("installed").and_then(serde_json::Value::as_bool) == Some(true)
    })
}

#[test]
fn target_qualified_toolchain_channel_fails_generation_by_name() -> Result<(), Box<dyn Error>> {
    // The generated resolver reads a bare X.Y.Z (hard decision 5), so
    // generation rejects a target-qualified channel instead of every job.
    let tmp = fresh_tempdir("tc-toml-target-qualified")?;
    setup_repo(&tmp, &config_with_suites("", ""))?;
    fs::write(
        tmp.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.94.1-x86_64-unknown-linux-gnu\"\n",
    )?;
    commit_all(&tmp)?;
    let out = spawn(&["generate"], &[], &tmp)?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(code(&out), 1, "{stderr}");
    assert!(
        stderr.contains("project_tool_version_not_exact:tool=rust,file=rust-toolchain.toml"),
        "{stderr}"
    );
    cleanup(&tmp);
    Ok(())
}

#[test]
fn rust_toolchain_toml_and_legacy_file_override_rust_and_omit_rustup_toolchain()
-> Result<(), Box<dyn Error>> {
    for (label, filename, contents, extra_rust) in [
        (
            "toml-with-matching-config",
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.94.1\"\ncomponents = [\"rustfmt\", \"clippy\"]\n",
            "toolchain = \"1.94.1\"\n",
        ),
        ("legacy-plain-file", "rust-toolchain", "1.94.1\n", ""),
    ] {
        let tmp = fresh_tempdir(&format!("tc-{label}"))?;
        let cfg = config_with_suites(extra_rust, "");
        setup_repo(&tmp, &cfg)?;
        fs::write(tmp.join(filename), contents)?;
        commit_all(&tmp)?;

        let out = spawn(&["generate"], &[], &tmp)?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !mise_has_rust("1.94.1") {
            assert_eq!(
                code(&out),
                1,
                "{label} generate must fail closed without the repo pin: {stderr}"
            );
            assert!(
                stderr.contains("missing_tool_pin: rust@1.94.1 not installed"),
                "{label} generate must name the unsatisfied pin: {stderr}"
            );
            eprintln!(
                "SKIPPED (pin absent): {label} generated-workflow assertions need \
                 rust@1.94.1 installed; install it with `mise install rust@1.94.1` \
                 to run the full coverage branch"
            );
            cleanup(&tmp);
            continue;
        }

        assert_eq!(code(&out), 0, "{label} generate stderr: {stderr}");

        let ci_yaml = fs::read_to_string(tmp.join(".github/workflows/ci.yml"))?;
        let suites_yaml = fs::read_to_string(tmp.join(".github/workflows/suites.yml"))?;
        let manifest_json =
            fs::read_to_string(tmp.join(".github/velnor/task-execution-manifest-v1.json"))?;

        for needle in [
            "rust@1.94.1",
            "rustup component add --toolchain 1.94.1-x86_64-unknown-linux-gnu clippy rustfmt",
        ] {
            assert!(
                ci_yaml.contains(needle),
                "{label} missing {needle:?} in ci.yml:\n{ci_yaml}"
            );
        }
        assert!(
            suites_yaml.contains("rust@1.94.1"),
            "{label} missing rust@1.94.1 in suites.yml:\n{suites_yaml}"
        );
        assert!(
            !ci_yaml.contains("RUSTUP_TOOLCHAIN"),
            "{label} ci.yml must omit RUSTUP_TOOLCHAIN when {filename} exists:\n{ci_yaml}"
        );
        assert!(
            !suites_yaml.contains("RUSTUP_TOOLCHAIN"),
            "{label} suites.yml must omit RUSTUP_TOOLCHAIN when {filename} exists:\n{suites_yaml}"
        );
        assert!(
            !manifest_json.contains("RUSTUP_TOOLCHAIN"),
            "{label} manifest must omit RUSTUP_TOOLCHAIN when {filename} exists:\n{manifest_json}"
        );
        cleanup(&tmp);
    }
    Ok(())
}

#[test]
fn config_only_rust_toolchain_exports_rustup_toolchain() -> Result<(), Box<dyn Error>> {
    for (label, extra_rust, extra_root) in [
        ("stacks-rust-toolchain", "toolchain = \"1.94.1\"\n", ""),
        ("tools-rust", "", "\n[tools]\nrust = \"1.94.1\"\n"),
    ] {
        let tmp = fresh_tempdir(&format!("tc-cfg-{label}"))?;
        let cfg = config_with_suites(extra_rust, extra_root);
        setup_repo(&tmp, &cfg)?;
        commit_all(&tmp)?;

        let out = spawn(&["generate"], &[], &tmp)?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        if label == "toml-target-qualified" {
            // The generated resolver reads a bare X.Y.Z (hard decision 5), so
            // generation rejects a target-qualified channel by name.
            assert_eq!(code(&out), 1, "{label}: {stderr}");
            assert!(
                stderr
                    .contains("project_tool_version_not_exact:tool=rust,file=rust-toolchain.toml"),
                "{label}: {stderr}"
            );
            cleanup(&tmp);
            continue;
        }
        if !mise_has_rust("1.94.1") {
            assert_eq!(
                code(&out),
                1,
                "{label} generate must fail closed without the repo pin: {stderr}"
            );
            assert!(
                stderr.contains("missing_tool_pin: rust@1.94.1 not installed"),
                "{label} generate must name the unsatisfied pin: {stderr}"
            );
            eprintln!(
                "SKIPPED (pin absent): {label} generated-workflow assertions need \
                 rust@1.94.1 installed; install it with `mise install rust@1.94.1` \
                 to run the full coverage branch"
            );
            cleanup(&tmp);
            continue;
        }

        assert_eq!(code(&out), 0, "{label} generate stderr: {stderr}");

        let ci_yaml = fs::read_to_string(tmp.join(".github/workflows/ci.yml"))?;
        let suites_yaml = fs::read_to_string(tmp.join(".github/workflows/suites.yml"))?;
        let manifest_json =
            fs::read_to_string(tmp.join(".github/velnor/task-execution-manifest-v1.json"))?;

        assert!(ci_yaml.contains("rust@1.94.1"), "{label} ci.yml");
        assert!(
            ci_yaml.contains("RUSTUP_TOOLCHAIN: 1.94.1")
                || ci_yaml.contains("RUSTUP_TOOLCHAIN: \"1.94.1\""),
            "{label} ci.yml must set RUSTUP_TOOLCHAIN:\n{ci_yaml}"
        );
        assert!(suites_yaml.contains("rust@1.94.1"), "{label} suites.yml");
        assert!(
            suites_yaml.contains("RUSTUP_TOOLCHAIN: 1.94.1")
                || suites_yaml.contains("RUSTUP_TOOLCHAIN: \"1.94.1\""),
            "{label} suites.yml must set RUSTUP_TOOLCHAIN:\n{suites_yaml}"
        );
        assert!(
            manifest_json.contains("\"RUSTUP_TOOLCHAIN\":\"1.94.1\""),
            "{label} manifest must set RUSTUP_TOOLCHAIN:\n{manifest_json}"
        );
        cleanup(&tmp);
    }
    Ok(())
}

#[test]
fn conflicting_or_non_exact_toolchain_file_fails_closed() -> Result<(), Box<dyn Error>> {
    for (label, extra_rust, extra_root) in [
        ("stacks-conflict", "toolchain = \"1.98.1\"\n", ""),
        ("tools-conflict", "", "\n[tools]\nrust = \"1.98.1\"\n"),
    ] {
        let tmp = fresh_tempdir(&format!("tc-{label}"))?;
        let cfg = config_with_suites(extra_rust, extra_root);
        setup_repo(&tmp, &cfg)?;
        fs::write(
            tmp.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.94.1\"\n",
        )?;
        commit_all(&tmp)?;

        for subcmd in ["plan", "generate"] {
            let out = spawn(&[subcmd], &[], &tmp)?;
            assert_eq!(code(&out), 1, "{label} {subcmd} must exit 1");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains("conflicting_rust_toolchain"),
                "{label} {subcmd} stderr must contain conflicting_rust_toolchain: {stderr}"
            );
        }
        cleanup(&tmp);
    }

    for (label, channel) in [
        ("stable", "stable"),
        ("two-part", "1.99"),
        ("nightly-date", "nightly-2026-10-01"),
    ] {
        let tmp = fresh_tempdir(&format!("tc-nonexact-{label}"))?;
        let cfg = config_with_suites("", "");
        setup_repo(&tmp, &cfg)?;
        fs::write(
            tmp.join("rust-toolchain.toml"),
            format!("[toolchain]\nchannel = \"{channel}\"\n"),
        )?;
        commit_all(&tmp)?;

        for subcmd in ["plan", "generate"] {
            let out = spawn(&[subcmd], &[], &tmp)?;
            assert_eq!(code(&out), 1, "{label} {subcmd} must exit 1");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains(&format!("invalid_rust_toolchain:{channel}")),
                "{label} {subcmd} stderr must contain invalid_rust_toolchain:{channel}: {stderr}"
            );
        }
        cleanup(&tmp);
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn tools_table_overrides_versions_across_generated_workflows() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("tc-tools-overrides")?;
    let extra_root = r#"
[tools]
cargo-nextest = "0.9.150"
jactionlint = "2.3.1"
shellcheck = "0.11.1"
gh = "2.103.0"
"#;
    let cfg = config_with_suites("", extra_root);
    setup_repo(&tmp, &cfg)?;
    commit_all(&tmp)?;
    let mise_bin = toolchain_fixture::installed_rust_mise_fixture(&tmp)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let fixture_path = std::env::join_paths(
        std::iter::once(mise_bin.as_os_str().to_owned())
            .chain(std::env::split_paths(&path).map(std::path::PathBuf::into_os_string)),
    )?;
    let fixture_path = fixture_path.to_str().ok_or("non-UTF-8 fixture PATH")?;
    let child_env = [("PATH", fixture_path)];

    let plan_out = spawn(&["plan"], &child_env, &tmp)?;
    assert_eq!(
        code(&plan_out),
        0,
        "plan stderr: {}\n{}\n{}",
        String::from_utf8_lossy(&plan_out.stderr),
        tool_context::diagnostic(),
        mise_failure_diagnostics(&tmp, MiseDiagnosticContext::Inherited, &child_env, None)
    );
    let plan_stdout = String::from_utf8_lossy(&plan_out.stdout);
    assert!(
        plan_stdout.contains("Jactionlint: 2.3.1 pinned, profile correctness"),
        "plan output must report overridden jactionlint version:\n{plan_stdout}"
    );

    let gen_out = spawn(&["generate"], &child_env, &tmp)?;
    assert_eq!(
        code(&gen_out),
        0,
        "generate stderr: {}\n{}\n{}",
        String::from_utf8_lossy(&gen_out.stderr),
        tool_context::diagnostic(),
        mise_failure_diagnostics(&tmp, MiseDiagnosticContext::Inherited, &child_env, None)
    );

    let ci_yaml = fs::read_to_string(tmp.join(".github/workflows/ci.yml"))?;
    let suites_yaml = fs::read_to_string(tmp.join(".github/workflows/suites.yml"))?;

    for needle in [
        "rust@1.99.1",
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.150",
        "jactionlint@2.3.1",
        "shellcheck@0.11.1",
        "gh@2.103.0",
    ] {
        assert!(
            ci_yaml.contains(needle),
            "missing {needle:?} in ci.yml:\n{ci_yaml}"
        );
    }
    assert!(
        suites_yaml.contains("rust@1.99.1"),
        "missing default Rust pin in suites.yml:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("aqua:nextest-rs/nextest/cargo-nextest@0.9.150"),
        "missing overridden cargo-nextest in suites.yml:\n{suites_yaml}"
    );

    cleanup(&tmp);
    Ok(())
}
