// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn qualification_staging_preserves_complete_trusted_source()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("python3")
        .args(["-I", "-S", "-B"])
        .arg(root.join("tests/release_qualification_source_test.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn budget_capture_binds_job_identity_and_original_start() -> Result<(), Box<dyn std::error::Error>>
{
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("python3")
        .arg(root.join("tests/release_budget_fixture.py"))
        .env("BUDGET_SCRIPT", root.join("src/release_budget.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn hosted_containment_requires_expected_result_and_empty_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("python3")
        .arg(root.join("tests/release_budget_systemd_logic.py"))
        .arg(root.join("tests/release_budget_systemd_fixture.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn hosted_supervisor_runs_candidate_scenarios_and_controller_regressions()
-> Result<(), Box<dyn std::error::Error>> {
    let release = std::fs::read_to_string("/etc/os-release")?;
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        return Err("hosted supervisor qualification requires Linux x86_64".into());
    }
    assert!(release.lines().any(|line| line == "ID=ubuntu"));
    assert!(release.lines().any(|line| line == "VERSION_ID=\"26.04\""));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for (file, arguments) in [
        ("release_bootstrap_watch_systemd_test.py", &[][..]),
        ("release_manager_scenario_controller_test.py", &[][..]),
        ("release_manager_home_access_test.py", &[][..]),
        ("release_source_names_test.py", &[][..]),
        (
            "release_manager_systemd_test.py",
            &["RealManagerTest.test_reobserved_cgroup_cannot_change_invocation"][..],
        ),
        ("release_manager_qualification.py", &["--execute"][..]),
    ] {
        let status = std::process::Command::new("/usr/bin/sudo")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/nonexistent")
            .env("LANG", "C")
            .args([
                "-n",
                "/usr/bin/env",
                "-i",
                "PATH=/usr/bin:/bin",
                "HOME=/nonexistent",
                "LANG=C",
                "/usr/bin/python3",
                "-I",
                "-S",
                "-B",
            ])
            .arg(root.join(file))
            .args(arguments)
            .status()?;
        assert!(
            status.success(),
            "hosted supervisor qualification failed: {file}"
        );
    }
    hosted_nested_workspace_qualification(&root)?;
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace root")?;
    let result = std::process::Command::new("cargo")
        .args([
            "nextest",
            "run",
            "--locked",
            "-p",
            "velnor-actions-cli",
            "--test",
            "crate_release_prepare_pr_public",
            "--run-ignored",
            "only",
            "-E",
            "test(hosted_managed_publisher_export_is_bounded_and_quiescent)",
            "--no-tests",
            "fail",
        ])
        .current_dir(workspace)
        .status()?;
    assert!(
        result.success(),
        "hosted managed-export qualification failed"
    );
    Ok(())
}

fn hosted_nested_workspace_qualification(
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let version = std::process::Command::new("rustc")
        .arg("--version")
        .output()?;
    if !version.status.success()
        || !std::str::from_utf8(&version.stdout)?.starts_with("rustc 1.99.1 ")
    {
        return Err("qualified Rust 1.99.1 is unavailable".into());
    }
    let output = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()?;
    if !output.status.success() {
        return Err("pinned Rust toolchain is unavailable".into());
    }
    let toolchain = std::str::from_utf8(&output.stdout)?.trim();
    if !std::path::Path::new(toolchain).is_absolute() {
        return Err("Rust toolchain path is not absolute".into());
    }
    for fault in ["", "git-lock", "outside-workspace"] {
        let status = std::process::Command::new("/usr/bin/sudo")
            .env_clear()
            .args([
                "-n",
                "/usr/bin/env",
                "-i",
                "PATH=/usr/bin:/bin",
                "HOME=/nonexistent",
                "LANG=C",
                &format!("VELNOR_SOURCE_TEST_TOOLCHAIN={toolchain}"),
                "VELNOR_TEST_PINNED_GIT=/usr/bin/git",
                "VELNOR_TEST_PINNED_GIT_HTTP=/usr/lib/git-core/git-remote-http",
                "VELNOR_TEST_SEMVER=/usr/bin/false",
                &format!("VELNOR_SOURCE_TEST_FAULT={fault}"),
                "/usr/bin/python3",
                "-I",
                "-S",
                "-B",
            ])
            .arg(root.join("release_source_nested_workspace_systemd_test.py"))
            .status()?;
        if !status.success() {
            return Err(format!("nested source qualification failed: {fault}").into());
        }
    }
    Ok(())
}

#[test]
fn publisher_step_fits_github_limit_and_binds_generator_pins()
-> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine as _;
    use std::io::{Read as _, Write as _};
    use velnor_actions_contract::StepKind;
    use velnor_actions_workflow_renderer::{
        join_argv_for_run, release_root_publisher::publisher_step,
    };
    let source = "a".repeat(40);
    let manifest = "b".repeat(64);
    for bootstrap in [false, true] {
        let step = publisher_step(bootstrap, "0.1.75", &source, &manifest)?;
        let StepKind::Shell { run, env } = step.kind else {
            return Err("publisher step is not shell".into());
        };
        let command = join_argv_for_run(&run)?;
        assert!(command.len() <= 21_000);
        let mut shell = std::process::Command::new("/bin/bash")
            .arg("-n")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        shell
            .stdin
            .take()
            .ok_or("missing shell stdin")?
            .write_all(command.as_bytes())?;
        assert!(
            shell.wait()?.success(),
            "generated publisher command is invalid shell"
        );
        let encoded = env
            .get("VELNOR_RELEASE_BOOTSTRAP_PAYLOAD")
            .ok_or("missing literal bundle")?;
        assert!(encoded.len() <= 32 * 1024);
        let compressed = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        let mut decoder = lzma_rust2::XzReader::new(compressed.as_slice(), false);
        let mut payload = Vec::new();
        decoder.read_to_end(&mut payload)?;
        let bundle: serde_json::Value = serde_json::from_slice(&payload)?;
        export_contract::assert_bootstrap_sources(&bundle);
        assert_eq!(run.last().map(String::as_str), Some(manifest.as_str()));
        assert_eq!(
            run.get(run.len() - 2).map(String::as_str),
            Some(source.as_str())
        );
        assert_eq!(env.contains_key("CARGO_REGISTRY_TOKEN"), bootstrap);
        assert_eq!(
            env.get("VELNOR_RELEASE_BUDGET").map(String::as_str),
            Some("${{ steps.release-budget.outputs.budget }}")
        );
        assert_bootstrap_transport(&run, encoded)?;
    }
    assert!(publisher_step(false, "0.1.75", "invalid", &manifest).is_err());
    Ok(())
}

fn assert_bootstrap_transport(
    run: &[String],
    encoded: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        let clock = std::process::Command::new(&run[3])
            .args([
                "-I",
                "-c",
                "import time; print(time.clock_gettime_ns(time.CLOCK_BOOTTIME)+5_000_000_000)",
            ])
            .env_clear()
            .output()?;
        assert!(clock.status.success());
        let deadline = String::from_utf8(clock.stdout)?;
        let rejected = std::process::Command::new(&run[3])
            .args(&run[4..])
            .env_clear()
            .env("VELNOR_RELEASE_BOOTSTRAP_PAYLOAD", encoded)
            .env("GITHUB_RUN_ID", "1")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_JOB", "fixture")
            .env(
                "VELNOR_RELEASE_BUDGET",
                format!("v1:1:1:fixture:{}", deadline.trim()),
            )
            .output()?;
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("publisher first stage rejected")
        );
    }
    for invalid in [
        None,
        Some("A".repeat(33 * 1024)),
        Some(format!("A{encoded}")),
    ] {
        let mut child = std::process::Command::new(&run[3]);
        child.args(&run[4..]).env_clear();
        if let Some(value) = invalid {
            child.env("VELNOR_RELEASE_BOOTSTRAP_PAYLOAD", value);
        }
        let result = child.output()?;
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("release bootstrap payload rejected")
        );
    }
    Ok(())
}

#[test]
fn generated_shell_arguments_preserve_apostrophes() -> Result<(), Box<dyn std::error::Error>> {
    for value in ["'", "''", "a'b", "a'b c", "'leading", "trailing'"] {
        let command = velnor_actions_workflow_renderer::join_argv_for_run(&[
            "/usr/bin/printf".to_owned(),
            "%s".to_owned(),
            value.to_owned(),
        ])?;
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &command])
            .env_clear()
            .output()?;
        assert!(
            output.status.success(),
            "generated shell failed for {value:?}"
        );
        assert_eq!(output.stdout, value.as_bytes());
    }
    Ok(())
}

#[test]
fn package_names_match_cargo_contract() {
    use velnor_actions_workflow_renderer::release_spec::validate_package_name;
    assert!(validate_package_name(&"a".repeat(64)).is_ok());
    assert!(validate_package_name(&"a".repeat(65)).is_err());
    for (name, valid) in [
        ("_codec", true),
        ("alpha-2", true),
        ("2codec", false),
        ("-codec", false),
        ("", false),
        ("é", false),
    ] {
        assert_eq!(validate_package_name(name).is_ok(), valid, "{name:?}");
    }
}

#[test]
fn renderer_and_release_contract_accept_the_same_environment_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    use velnor_actions_workflow_renderer::release_spec::validate_environment;
    let base = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
    for (name, valid) in [
        ("a".repeat(128), true),
        ("a".repeat(129), true),
        ("a".repeat(255), true),
        ("a".repeat(256), false),
        ("prod/./release".into(), false),
        ("prod//release".into(), false),
        ("..".into(), false),
    ] {
        let mut value = base.clone();
        value["environment"] = name.clone().into();
        assert_eq!(
            serde_json::from_value::<RustReleaseConfig>(value)?
                .validate("fixture")
                .is_ok(),
            valid,
            "contract {name}"
        );
        assert_eq!(
            validate_environment(&name).is_ok(),
            valid,
            "renderer {name}"
        );
    }
    Ok(())
}

#[path = "release_export_contract_public.rs"]
mod export_contract;
