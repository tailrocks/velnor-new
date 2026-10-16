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

[workflow.verify]
jobs = ["zizmor"]

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
        assert_eq!(
            code(&out),
            0,
            "{label} generate stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );

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
        assert_eq!(
            code(&out),
            0,
            "{label} generate stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );

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
fn conflicting_toolchain_file_and_config_fails_closed() -> Result<(), Box<dyn Error>> {
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
    Ok(())
}

#[test]
fn tools_table_overrides_versions_across_generated_workflows() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("tc-tools-overrides")?;
    let extra_root = r#"
[tools]
cargo-nextest = "0.9.150"
actionlint = "1.7.13"
shellcheck = "0.11.1"
zizmor = "1.31.0"
gh = "2.103.0"
"#;
    let cfg = config_with_suites("", extra_root);
    setup_repo(&tmp, &cfg)?;
    commit_all(&tmp)?;

    let plan_out = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(
        code(&plan_out),
        0,
        "plan stderr: {}",
        String::from_utf8_lossy(&plan_out.stderr)
    );
    let plan_stdout = String::from_utf8_lossy(&plan_out.stdout);
    assert!(
        plan_stdout.contains("Actionlint: 1.7.13 pinned"),
        "plan output must report overridden actionlint version:\n{plan_stdout}"
    );

    let gen_out = spawn(&["generate"], &[], &tmp)?;
    assert_eq!(
        code(&gen_out),
        0,
        "generate stderr: {}",
        String::from_utf8_lossy(&gen_out.stderr)
    );

    let ci_yaml = fs::read_to_string(tmp.join(".github/workflows/ci.yml"))?;
    let suites_yaml = fs::read_to_string(tmp.join(".github/workflows/suites.yml"))?;

    for needle in [
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.150",
        "actionlint@1.7.13",
        "shellcheck@0.11.1",
        "zizmor@1.31.0",
        "gh@2.103.0",
    ] {
        assert!(
            ci_yaml.contains(needle),
            "missing {needle:?} in ci.yml:\n{ci_yaml}"
        );
    }
    assert!(
        suites_yaml.contains("aqua:nextest-rs/nextest/cargo-nextest@0.9.150"),
        "missing overridden cargo-nextest in suites.yml:\n{suites_yaml}"
    );

    cleanup(&tmp);
    Ok(())
}
